#!/usr/bin/env python3
"""
Raqim v0.2.0 — 5-Minute OpenAI Base-URL Swap Quickstart

Zero Raqim SDK required inside your agent code:
Point ANY OpenAI client (`openai`, `httpx`, `fetch`, `curl`) at Raqim using:
  OPENAI_BASE_URL=http://127.0.0.1:8081/v1/egress/openai/v1
  OPENAI_API_KEY=<rqm_v1 workload token from `raqim-cli keys forge`>

Run self-contained local verification (starts an isolated mock OpenAI + daemon):
  python3 quickstart.py --self-test

Or point at an already-running Raqim gateway:
  OPENAI_BASE_URL=http://127.0.0.1:8081/v1/egress/openai/v1 \
  OPENAI_API_KEY=$(cat ../../ca-keys/dev_bot.token) \
  python3 quickstart.py
"""

import argparse
import json
import os
import shutil
import socket
import subprocess
import sys
import tempfile
import threading
import time
import urllib.error
import urllib.request
from http.server import BaseHTTPRequestHandler, ThreadingHTTPServer

REPO_ROOT = os.path.abspath(os.path.join(os.path.dirname(__file__), "..", ".."))
MOCK_SECRET = "sk-proj-quickstart-daemon-only-canary"


def http_post(url: str, headers: dict, raw_body: bytes) -> tuple[int, dict, bytes]:
    req = urllib.request.Request(url, data=raw_body, headers=headers, method="POST")
    try:
        with urllib.request.urlopen(req, timeout=10) as resp:
            return resp.status, {k.lower(): v for k, v in resp.headers.items()}, resp.read()
    except urllib.error.HTTPError as exc:
        return exc.code, {k.lower(): v for k, v in exc.headers.items()}, exc.read()


def run_agent_flow(base_url: str, workload_token: str) -> None:
    endpoint = f"{base_url.rstrip('/')}/chat/completions"
    base_headers = {
        "Authorization": f"Bearer {workload_token}",
        "Content-Type": "application/json",
    }

    assert MOCK_SECRET not in "".join(os.environ.values()), "Real OpenAI key must NOT sit in agent env!"

    # 1. Valid request within policy (`max_tokens = 250 <= 1000`)
    st, _, body = http_post(
        endpoint,
        {**base_headers, "X-Raqim-Step": "1"},
        json.dumps({
            "model": "gpt-4o-mini",
            "max_tokens": 250,
            "user": "customer-secret-id",
            "messages": [{"role": "user", "content": "Summarize today's ledger status."}],
        }).encode(),
    )
    assert st == 200, f"Expected 200 on valid call, got {st}: {body.decode()}"
    print(f"[1/4 PASS] Valid chat completion -> HTTP {st} (real OpenAI key injected by daemon)")

    # 2. Streaming SSE call + instant deterministic $0.00 gateway replay
    sse_payload = json.dumps({
        "model": "gpt-4o-mini",
        "max_tokens": 120,
        "stream": True,
        "messages": [{"role": "user", "content": "Stream quick health note."}],
    }).encode()
    st_live, hdr_live, body_live = http_post(endpoint, {**base_headers, "X-Raqim-Step": "2"}, sse_payload)
    assert st_live == 200 and b"Hello from Raqim" in body_live, f"Streaming failed: {st_live} {body_live}"

    st_rep, hdr_rep, body_rep = http_post(
        endpoint,
        {**base_headers, "X-Raqim-Step": "2", "X-Raqim-Mode": "replay"},
        sse_payload,
    )
    assert st_rep == 200 and hdr_rep.get("x-raqim-replay") == "HIT" and body_rep == body_live
    print(f"[2/4 PASS] SSE stream + $0.00 Gateway Replay -> x-raqim-replay: {hdr_rep.get('x-raqim-replay')}")

    # 3. Over-cap prompt (`max_tokens = 5000 > 1000` limit) -> BLOCKED + QUARANTINED
    st_cap, _, body_cap = http_post(
        endpoint,
        base_headers,
        json.dumps({
            "model": "gpt-4o-mini",
            "max_tokens": 5000,
            "messages": [{"role": "user", "content": "Dump 5000 tokens."}],
        }).encode(),
    )
    assert st_cap == 403, f"Expected 403 on over-cap call, got {st_cap}"
    print(f"[3/4 BLOCKED] Over max_tokens cap -> HTTP {st_cap}: {body_cap.decode()}")

    # 4. Subsequent legitimate-looking call from quarantined token -> STILL BLOCKED
    st_post, _, body_post = http_post(
        endpoint,
        base_headers,
        json.dumps({
            "model": "gpt-4o-mini",
            "max_tokens": 50,
            "messages": [{"role": "user", "content": "Retry small call."}],
        }).encode(),
    )
    assert st_post == 403, f"Expected 403 after quarantine, got {st_post}"
    print(f"[4/4 BLOCKED] Post-quarantine retry -> HTTP {st_post}: {body_post.decode()}")


def run_self_test() -> None:
    core_bin = os.path.join(REPO_ROOT, "target", "release", "raqim-core")
    cli_bin = os.path.join(REPO_ROOT, "target", "release", "raqim-cli")
    if not os.path.exists(core_bin) or not os.path.exists(cli_bin):
        raise FileNotFoundError(
            "Build release binaries first: cargo build --release --bin raqim-core --bin raqim-cli"
        )

    upstream_hits = {"count": 0}

    class MockOpenAI(BaseHTTPRequestHandler):
        def log_message(self, *args):
            pass

        def do_POST(self):
            length = int(self.headers.get("Content-Length", 0))
            payload = json.loads(self.rfile.read(length))
            assert self.headers.get("Authorization") == f"Bearer {MOCK_SECRET}", "Daemon did not inject secret!"
            upstream_hits["count"] += 1
            if payload.get("stream"):
                self.send_response(200)
                self.send_header("Content-Type", "text/event-stream")
                self.end_headers()
                self.wfile.write(b'data: {"choices":[{"delta":{"content":"Hello from Raqim"}}]}\n\n')
                self.wfile.write(b"data: [DONE]\n\n")
            else:
                out = json.dumps({
                    "id": "chatcmpl-quickstart",
                    "choices": [{"message": {"role": "assistant", "content": "Hello from Raqim!"}}],
                }).encode()
                self.send_response(200)
                self.send_header("Content-Type", "application/json")
                self.send_header("Content-Length", str(len(out)))
                self.end_headers()
                self.wfile.write(out)

    server = ThreadingHTTPServer(("127.0.0.1", 0), MockOpenAI)
    upstream_port = server.server_address[1]
    threading.Thread(target=server.serve_forever, daemon=True).start()

    tcp_port = http_port = 0
    for port in range(19200, 19800, 2):
        s1, s2 = socket.socket(), socket.socket()
        try:
            s1.bind(("127.0.0.1", port))
            s2.bind(("127.0.0.1", port + 1))
            tcp_port, http_port = port, port + 1
            break
        except OSError:
            pass
        finally:
            s1.close()
            s2.close()

    sandbox = tempfile.mkdtemp(prefix="raqim_openai_qs_")
    os.makedirs(os.path.join(sandbox, "ca-keys"), exist_ok=True)
    os.makedirs(os.path.join(sandbox, "vault"), exist_ok=True)
    shutil.copyfile(os.path.join(os.path.dirname(__file__), "policy.toml"), os.path.join(sandbox, "policy.toml"))

    with open(os.path.join(os.path.dirname(__file__), "raqim.toml"), "r", encoding="utf-8") as f:
        cfg_text = f.read()
    cfg_text = cfg_text.replace("port = 8080", f"port = {tcp_port}").replace(
        "https://api.openai.com", f"http://127.0.0.1:{upstream_port}"
    )
    with open(os.path.join(sandbox, "raqim.toml"), "w", encoding="utf-8") as f:
        f.write(cfg_text)

    subprocess.run(
        [cli_bin, "keys", "forge", "--name", "dev_bot", "--group", "openai_agent"],
        cwd=sandbox,
        check=True,
        stdout=subprocess.DEVNULL,
    )
    with open(os.path.join(sandbox, "ca-keys", "dev_bot.token"), "r", encoding="utf-8") as f:
        token = f.read().strip()

    daemon_env = os.environ.copy()
    daemon_env["RAQIM_ENV"] = "production"
    daemon_env["OPENAI_API_KEY"] = MOCK_SECRET
    proc = subprocess.Popen([core_bin], cwd=sandbox, env=daemon_env, stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL)

    try:
        for _ in range(60):
            try:
                with urllib.request.urlopen(f"http://127.0.0.1:{http_port}/health", timeout=0.5) as r:
                    if r.status == 200:
                        break
            except Exception:
                time.sleep(0.05)

        run_agent_flow(f"http://127.0.0.1:{http_port}/v1/egress/openai/v1", token)
        assert upstream_hits["count"] == 2, f"Expected 2 upstream hits (1 normal + 1 live SSE; replay/blocks = 0), got {upstream_hits['count']}"

        doc = subprocess.run([cli_bin, "doctor"], cwd=sandbox, capture_output=True, text=True, check=True)
        pol = subprocess.run([cli_bin, "policy", "list"], cwd=sandbox, capture_output=True, text=True, check=True)
        assert "QUARAN" in pol.stdout.upper() or "openai" in pol.stdout.lower()
        print(f"\n[CLI doctor]\n{doc.stdout.strip()}")
        print(f"\n[CLI policy list]\n{pol.stdout.strip()}")
        print(f"\nSelf-test passed (upstream hits: {upstream_hits['count']}; replay + 2 blocked calls hit upstream 0 times).")
    finally:
        proc.terminate()
        try:
            proc.wait(timeout=3)
        except subprocess.TimeoutExpired:
            proc.kill()
        server.shutdown()
        shutil.rmtree(sandbox, ignore_errors=True)


def main() -> int:
    parser = argparse.ArgumentParser(description="Raqim OpenAI Base-URL Swap Quickstart")
    parser.add_argument("--self-test", action="store_true", help="Run against a temporary local daemon + mock OpenAI")
    args = parser.parse_args()

    if args.self_test:
        run_self_test()
        return 0

    base_url = os.environ.get("OPENAI_BASE_URL", "http://127.0.0.1:8081/v1/egress/openai/v1")
    token = os.environ.get("OPENAI_API_KEY", "")
    if not token.startswith("rqm_v1."):
        print("Pass OPENAI_API_KEY=<rqm_v1 token> or run: python3 quickstart.py --self-test", file=sys.stderr)
        return 1
    run_agent_flow(base_url, token)
    return 0


if __name__ == "__main__":
    sys.exit(main())
