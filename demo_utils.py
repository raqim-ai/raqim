import asyncio
import json
import os
import re
import shutil
import socket
import subprocess
import sys
import time
import uuid
import zlib
from dataclasses import dataclass, field
from typing import Any, Dict, List, Optional, Tuple

import blake3
import httpx

BASE_DIR = os.path.dirname(os.path.abspath(__file__))
SANDBOX_DIR = os.path.join(BASE_DIR, ".demo_sandbox")

DAEMON_PAYSTACK_SECRET = "sk_live_raqim_vault_991a7f"
DAEMON_LLM_SECRET = "AIza_raqim_vault_secret_882b"
PAYSTACK_ENV_VAR = "RAQIM_DEMO_PAYSTACK_KEY"
LLM_ENV_VAR = "RAQIM_DEMO_LLM_KEY"

ANSI_ESCAPE = re.compile(r"\x1B(?:[@-Z\\-_]|\[[0-?]*[ -/]*[@-~])")


def strip_ansi(s: str) -> str:
    return ANSI_ESCAPE.sub("", s)


def visible_len(s: str) -> int:
    return len(strip_ansi(s))


def pad_visible(s: str, width: int) -> str:
    v = visible_len(s)
    return s + (" " * max(0, width - v))


class Style:
    RESET = "\033[0m"
    BOLD = "\033[1m"
    DIM = "\033[2m"
    RED = "\033[91m"
    GREEN = "\033[92m"
    YELLOW = "\033[93m"
    BLUE = "\033[94m"
    MAGENTA = "\033[95m"
    CYAN = "\033[96m"
    WHITE = "\033[97m"


def _ensure_raqim_importable():
    py_dir = os.path.join(BASE_DIR, "raqim-py")
    if py_dir not in sys.path:
        sys.path.insert(0, py_dir)


_ensure_raqim_importable()


def resolve_binary(name: str) -> Tuple[str, str]:
    """Resolves `raqim-core` or `raqim-cli` from release build first, then debug."""
    rel = os.path.join(BASE_DIR, "target", "release", name)
    dbg = os.path.join(BASE_DIR, "target", "debug", name)
    if os.path.exists(rel):
        return rel, "Release Build (Rust Native)"
    if os.path.exists(dbg):
        return dbg, "Debug Build (Rust Native)"
    which = shutil.which(name)
    if which:
        return which, "System PATH"
    raise FileNotFoundError(
        f"Missing compiled binary '{name}'. Run: cargo build --release -p raqim-core -p raqim-cli"
    )


def find_free_port() -> int:
    with socket.socket(socket.AF_INET, socket.SOCK_STREAM) as s:
        s.bind(("127.0.0.1", 0))
        return int(s.getsockname()[1])


def find_free_port_pair(start_port: int = 18080) -> Tuple[int, int]:
    """Finds consecutive free ports (p, p+1) for Raqim TCP ingress + HTTP gateway."""
    for port in range(start_port, start_port + 500, 2):
        s1 = socket.socket(socket.AF_INET, socket.SOCK_STREAM)
        s2 = socket.socket(socket.AF_INET, socket.SOCK_STREAM)
        try:
            s1.bind(("127.0.0.1", port))
            s2.bind(("127.0.0.1", port + 1))
            return port, port + 1
        except OSError:
            continue
        finally:
            s1.close()
            s2.close()
    raise RuntimeError("No free consecutive port pair available")


def print_banner():
    print(f"""{Style.CYAN}{Style.BOLD}
  ____                  _
 |  _ \\  __ _   __ _   (_)  _ __ ___
 | |_) |/ _` | / _` |  | | | '_ ` _ \\
 |  _ <| (_| || (_| |  | | | | | | | |
 |_| \\_\\\\__,_| \\__, |  |_| |_| |_| |_|  v0.2.0
                  |_|
  HOSTILE-AGENT EGRESS CUSTODY & CRYPTOGRAPHIC AUDIT GATEWAY
{Style.RESET}""")


def print_header(title: str, subtitle: str = ""):
    w = 78
    print(f"\n{Style.BOLD}{Style.BLUE}{'=' * w}{Style.RESET}")
    print(f"{Style.BOLD}{Style.WHITE}  {title}{Style.RESET}")
    if subtitle:
        print(f"{Style.DIM}  {subtitle}{Style.RESET}")
    print(f"{Style.BOLD}{Style.BLUE}{'=' * w}{Style.RESET}")


def print_incident_card(
    actor: str,
    action: str,
    verdict_badge: str,
    http_status: Any,
    error_code: str,
    wire_message: str,
    story: str,
    outcome_lines: List[str],
    color: str = Style.RED,
):
    """Renders a structured story-telling forensic security card."""
    w = 76
    bar = f"{color}+{'-' * (w - 2)}+{Style.RESET}"
    print(f"\n{bar}")
    print(f"{color}|{Style.RESET} {pad_visible(f'{Style.BOLD}{verdict_badge}{Style.RESET}', w - 4)} {color}|{Style.RESET}")
    print(f"{color}+{'-' * (w - 2)}+{Style.RESET}")

    rows = [
        ("ACTOR", actor),
        ("ATTEMPT", action),
        ("HTTP / CODE", f"HTTP {http_status} | {error_code}"),
        ("WIRE ERROR", wire_message[: w - 20]),
        ("WHY IT MATTERS", story),
    ]
    for k, v in rows:
        words = v.split()
        line = ""
        first = True
        max_val = w - 21
        for word in words:
            if len(line) + len(word) + (1 if line else 0) <= max_val:
                line = f"{line} {word}".strip()
            else:
                prefix = f"{Style.DIM}{k:<14}:{Style.RESET} " if first else (" " * 16)
                print(f"{color}|{Style.RESET} {pad_visible(prefix + line, w - 4)} {color}|{Style.RESET}")
                first = False
                line = word
        prefix = f"{Style.DIM}{k:<14}:{Style.RESET} " if first else (" " * 16)
        print(f"{color}|{Style.RESET} {pad_visible(prefix + line, w - 4)} {color}|{Style.RESET}")

    for ol in outcome_lines:
        print(f"{color}|{Style.RESET} {pad_visible(f'{Style.BOLD}->{Style.RESET} {ol}', w - 4)} {color}|{Style.RESET}")
    print(bar)


async def beat(label: str = "", pause: bool = False, stream_note: Optional[str] = None, delay: float = 0.08):
    if stream_note:
        for tok in stream_note.split(" "):
            sys.stdout.write(f"{Style.CYAN}{tok} {Style.RESET}")
            sys.stdout.flush()
            await asyncio.sleep(delay)
        print()
    if pause:
        await asyncio.to_thread(input, f"\n{Style.DIM}[Enter to continue: {label}]{Style.RESET}")


@dataclass
class UpstreamCallRecord:
    connector: str
    route: str
    headers: Dict[str, str]
    body_text: str
    status_sent: int


@dataclass
class LocalMockUpstream:
    """In-process async HTTP/1.1 mock Paystack + Bank Settlement + LLM SSE server."""

    host: str = "127.0.0.1"
    port: int = field(default_factory=find_free_port)
    _server: Optional[asyncio.AbstractServer] = None
    paystack_calls: List[UpstreamCallRecord] = field(default_factory=list)
    flaky_received_keys: List[str] = field(default_factory=list)
    llm_calls_live: int = 0
    total_settled_ngn: float = 0.0
    auth_failures: int = 0

    @property
    def base_url(self) -> str:
        return f"http://{self.host}:{self.port}"

    async def start(self):
        self._server = await asyncio.start_server(self._handle_conn, self.host, self.port)

    async def stop(self):
        if self._server:
            self._server.close()
            await self._server.wait_closed()

    async def _handle_conn(self, reader: asyncio.StreamReader, writer: asyncio.StreamWriter):
        try:
            raw_head = await reader.readuntil(b"\r\n\r\n")
        except Exception:
            writer.close()
            return

        head_text = raw_head.decode("utf-8", errors="replace")
        lines = head_text.split("\r\n")
        request_line = lines[0].split(" ")
        method = request_line[0]
        path = request_line[1] if len(request_line) > 1 else "/"

        headers: Dict[str, str] = {}
        for line in lines[1:]:
            if ":" in line:
                k, v = line.split(":", 1)
                headers[k.strip().lower()] = v.strip()

        content_len = int(headers.get("content-length", "0"))
        body_bytes = await reader.readexactly(content_len) if content_len > 0 else b""
        body_text = body_bytes.decode("utf-8", errors="replace")

        try:
            if path.startswith("/paystack/transfer_flaky"):
                auth = headers.get("authorization", "")
                if auth != f"Bearer {DAEMON_PAYSTACK_SECRET}":
                    self.auth_failures += 1
                    await self._send_json(writer, 401, {"error": "invalid vault secret"})
                    return
                idem_key = headers.get("idempotency-key", "")
                self.flaky_received_keys.append(idem_key)
                self.paystack_calls.append(
                    UpstreamCallRecord("paystack", path, headers, body_text, 0)
                )
                # Simulate abrupt TCP drop AFTER receiving transfer intent -> forces IN_DOUBT!
                writer.close()
                await writer.wait_closed()
                return

            if path.startswith("/paystack/"):
                auth = headers.get("authorization", "")
                if auth != f"Bearer {DAEMON_PAYSTACK_SECRET}":
                    self.auth_failures += 1
                    await self._send_json(writer, 401, {"error": "invalid vault secret"})
                    return
                idem_key = headers.get("idempotency-key", "none")
                payload = json.loads(body_text) if body_text else {}
                amt = float(payload.get("amount", 0))
                self.total_settled_ngn += amt
                resp = {
                    "status": True,
                    "message": "Transfer settled via Paystack Mock Rail",
                    "data": {
                        "reference": f"PSK-{uuid.uuid4().hex[:10].upper()}",
                        "amount_ngn": amt,
                        "currency": payload.get("currency", "NGN"),
                        "recipient": payload.get("recipient", "unknown"),
                        "idempotency_key_seen": idem_key,
                        "auth_verified": True,
                    },
                }
                self.paystack_calls.append(
                    UpstreamCallRecord("paystack", path, headers, body_text, 200)
                )
                await self._send_json(writer, 200, resp)
                return

            if path.startswith("/llm/stream"):
                if headers.get("x-api-key", "") != DAEMON_LLM_SECRET:
                    self.auth_failures += 1
                    await self._send_json(writer, 401, {"error": "missing x-api-key"})
                    return
                self.llm_calls_live += 1
                chunks = [
                    {"step": 1, "verdict": "SCANNING", "detail": "Invoice VENDOR-NG-442 verified against CAC registry."},
                    {"step": 2, "verdict": "PII_CHECK", "detail": "Customer BVN redacted before persistent storage."},
                    {"step": 3, "verdict": "AML_PASS", "detail": "Rolling 1h exposure within NGN 100,000 policy ceiling."},
                    {"step": 4, "verdict": "DONE", "risk_score": 0.08, "action": "APPROVE_SINGLE_TRANCHE"},
                ]
                sse_body = "".join(f"data: {json.dumps(c)}\n\n" for c in chunks).encode("utf-8")
                resp_head = (
                    "HTTP/1.1 200 OK\r\n"
                    "Content-Type: text/event-stream\r\n"
                    "Cache-Control: no-cache\r\n"
                    f"Content-Length: {len(sse_body)}\r\n"
                    "Connection: close\r\n\r\n"
                ).encode("utf-8")
                writer.write(resp_head + sse_body)
                await writer.drain()
                writer.close()
                await writer.wait_closed()
                return

            await self._send_json(writer, 404, {"error": f"Unknown mock path: {method} {path}"})
        except Exception as exc:
            await self._send_json(writer, 500, {"error": str(exc)})

    async def _send_json(self, writer: asyncio.StreamWriter, status: int, obj: Dict[str, Any]):
        body = json.dumps(obj).encode("utf-8")
        reason = {200: "OK", 401: "Unauthorized", 404: "Not Found", 500: "Error"}.get(status, "Status")
        head = (
            f"HTTP/1.1 {status} {reason}\r\n"
            "Content-Type: application/json\r\n"
            f"Content-Length: {len(body)}\r\n"
            "Connection: close\r\n\r\n"
        ).encode("utf-8")
        writer.write(head + body)
        await writer.drain()
        writer.close()
        await writer.wait_closed()


@dataclass
class SandboxConfig:
    tcp_port: int
    http_port: int
    upstream_url: str
    daemon_bin: str
    cli_bin: str
    admin_socket: str = os.path.join(SANDBOX_DIR, "vault", "admin.sock")
    master_key_path: str = os.path.join(SANDBOX_DIR, "ca-keys", "swarm_master.key")
    admin_secret_path: str = os.path.join(SANDBOX_DIR, "ca-keys", "admin.secret")
    wal_path: str = os.path.join(SANDBOX_DIR, "production.wal")
    daemon_log_path: str = os.path.join(SANDBOX_DIR, "daemon.log")


def prepare_clean_sandbox(upstream_url: str) -> SandboxConfig:
    shutil.rmtree(SANDBOX_DIR, ignore_errors=True)
    for sub in ["", "vault", os.path.join("vault", "witnesses"), os.path.join("vault", "cold_segments"), "ca-keys", "agent_keys", "evidence"]:
        os.makedirs(os.path.join(SANDBOX_DIR, sub), exist_ok=True)

    tcp_port, http_port = find_free_port_pair(18080)
    daemon_bin, _ = resolve_binary("raqim-core")
    cli_bin, _ = resolve_binary("raqim-cli")

    raqim_toml = f"""tenant_id = "kobo_fintech_demo"
port = {tcp_port}
host = "127.0.0.1"
wal_path = "./production.wal"
manifest_path = "./compaction.manifest.json"
witness_path = "./vault/witnesses"
policy_path = "./policy.toml"
checkpoint_path = "./vault/state_checkpoint.bin"
control_journal_path = "./vault/control_journal.bin"
cold_store_path = "./vault/cold_segments"
admin_socket_path = "./vault/admin.sock"

[[connectors]]
name = "paystack"
upstream_url = "{upstream_url}/paystack"
secret_env = "{PAYSTACK_ENV_VAR}"
auth_header = "Authorization"
auth_prefix = "Bearer "
idempotency_header = "Idempotency-Key"
allowed_routes = ["POST /transfer", "POST /transfer_flaky", "GET /tx/*"]
redact_pointers = ["/customer/bvn", "/card/cvv"]

[[connectors]]
name = "llm_upstream"
upstream_url = "{upstream_url}/llm"
secret_env = "{LLM_ENV_VAR}"
auth_header = "x-api-key"
auth_prefix = ""
allowed_routes = ["POST /stream"]
redact_pointers = []
"""
    with open(os.path.join(SANDBOX_DIR, "raqim.toml"), "w", encoding="utf-8") as f:
        f.write(raqim_toml)

    policy_toml = """[groups.admin_group]
allowed_namespaces = ["*"]
blocked_namespaces = []
max_tps = 10000
burst_capacity = 5000

[groups.fintech_treasury]
allowed_namespaces = ["/egress/paystack/*", "/egress/llm_upstream/*", "/fintech/*", "/audit/*"]
blocked_namespaces = ["/finance/restricted/*", "/admin/*"]
max_tps = 1000
burst_capacity = 200

[[groups.fintech_treasury.predicates]]
namespace = "/egress/paystack/transfer"
pointer = "/amount"
required = true
min_value = 100.0
max_value = 50000.0
max_hourly_sum = 100000.0

[[groups.fintech_treasury.predicates]]
namespace = "/egress/paystack/transfer"
pointer = "/currency"
required = true
allowed_values = ["NGN"]

[[groups.fintech_treasury.predicates]]
namespace = "/egress/paystack/transfer_flaky"
pointer = "/amount"
required = true
min_value = 100.0
max_value = 50000.0
"""
    with open(os.path.join(SANDBOX_DIR, "policy.toml"), "w", encoding="utf-8") as f:
        f.write(policy_toml)

    return SandboxConfig(
        tcp_port=tcp_port,
        http_port=http_port,
        upstream_url=upstream_url,
        daemon_bin=daemon_bin,
        cli_bin=cli_bin,
    )


async def wait_daemon_ready(cfg: SandboxConfig, timeout_s: float = 12.0) -> float:
    t0 = time.perf_counter()
    url = f"http://127.0.0.1:{cfg.http_port}/health"
    async with httpx.AsyncClient(timeout=0.8) as client:
        while time.perf_counter() - t0 < timeout_s:
            if os.path.exists(cfg.admin_socket):
                try:
                    r = await client.get(url)
                    if r.status_code == 200:
                        return (time.perf_counter() - t0) * 1000.0
                except Exception:
                    pass
            await asyncio.sleep(0.05)
    raise TimeoutError(f"raqim-core daemon did not become ready within {timeout_s}s; see {cfg.daemon_log_path}")


def start_daemon(cfg: SandboxConfig) -> subprocess.Popen:
    env = os.environ.copy()
    env[PAYSTACK_ENV_VAR] = DAEMON_PAYSTACK_SECRET
    env[LLM_ENV_VAR] = DAEMON_LLM_SECRET
    env["RAQIM_ENV"] = "production"

    log_file = open(cfg.daemon_log_path, "ab")
    return subprocess.Popen(
        [cfg.daemon_bin],
        cwd=SANDBOX_DIR,
        env=env,
        stdout=log_file,
        stderr=subprocess.STDOUT,
    )


def stop_daemon(proc: Optional[subprocess.Popen]):
    if proc and proc.poll() is None:
        proc.terminate()
        try:
            proc.wait(timeout=3)
        except subprocess.TimeoutExpired:
            proc.kill()


async def phoenix_kill_and_resurrect(proc: subprocess.Popen, cfg: SandboxConfig) -> Tuple[subprocess.Popen, float]:
    if proc.poll() is None:
        proc.kill()
        proc.wait(timeout=3)
    if os.path.exists(cfg.admin_socket):
        try:
            os.remove(cfg.admin_socket)
        except OSError:
            pass
    t0 = time.perf_counter()
    new_proc = start_daemon(cfg)
    await wait_daemon_ready(cfg)
    elapsed_ms = (time.perf_counter() - t0) * 1000.0
    return new_proc, elapsed_ms


def forge_offline_agent_identity(cfg: SandboxConfig, alias: str, group_name: str, ttl_seconds: int = 3600) -> Dict[str, str]:
    from raqim.raqim_core import RaqimCryptoCore

    key_path = os.path.join(SANDBOX_DIR, "agent_keys", f"{alias}.pem")
    cert_path = os.path.join(SANDBOX_DIR, "agent_keys", f"{alias}.cert")
    with open(key_path, "wb") as f:
        f.write(os.urandom(32))
    os.chmod(key_path, 0o600)

    core = RaqimCryptoCore(key_path)
    agent_hex = blake3.blake3(
        core.pub_key_bytes, derive_key_context="raqim.agent.v1.identity"
    ).digest(length=16).hex()

    cert_bytes = RaqimCryptoCore.forge_certificate_from_master(
        cfg.master_key_path, agent_hex, group_name, ttl_seconds
    )
    with open(cert_path, "wb") as f:
        f.write(cert_bytes)

    token = RaqimCryptoCore.forge_workload_token_from_master(
        cfg.master_key_path, agent_hex, group_name, ttl_seconds
    )
    return {
        "alias": alias,
        "agent_hex": agent_hex,
        "key_path": key_path,
        "cert_path": cert_path,
        "workload_token": token,
    }


async def admin_uds_request(cfg: SandboxConfig, method: str, path: str, json_body: Optional[Dict[str, Any]] = None) -> httpx.Response:
    with open(cfg.admin_secret_path, "r", encoding="utf-8") as f:
        secret = f.read().strip()
    transport = httpx.AsyncHTTPTransport(uds=cfg.admin_socket)
    async with httpx.AsyncClient(transport=transport, timeout=5.0) as client:
        return await client.request(
            method.upper(),
            f"http://localhost{path}",
            headers={"Authorization": f"Bearer {secret}"},
            json=json_body,
        )


def run_cli(cfg: SandboxConfig, args: List[str]) -> Tuple[int, str, str]:
    cmd = [
        cfg.cli_bin,
        "--admin-socket",
        "./vault/admin.sock",
        "--admin-secret-path",
        "./ca-keys/admin.secret",
        "--data-url",
        f"http://127.0.0.1:{cfg.http_port}",
        *args,
    ]
    res = subprocess.run(cmd, cwd=SANDBOX_DIR, capture_output=True, text=True, timeout=10)
    return res.returncode, res.stdout.strip(), res.stderr.strip()


def verify_wal_crc_tamper_detection(wal_path: str) -> Tuple[bool, str, int]:
    """Copies the live WAL, flips 1 payload bit, and verifies CRC32 frame detection."""
    if not os.path.exists(wal_path) or os.path.getsize(wal_path) < 24:
        return False, "WAL missing or too small", 0

    with open(wal_path, "rb") as f:
        raw = bytearray(f.read())

    # WAL Frame layout (raqim-core/src/main.rs:343-360): [len:4 LE][crc:4 LE][payload:len]
    entry_len = int.from_bytes(raw[0:4], "little")
    stored_crc = int.from_bytes(raw[4:8], "little")
    payload = bytes(raw[8 : 8 + entry_len])
    computed_orig = zlib.crc32(payload) & 0xFFFFFFFF
    if computed_orig != stored_crc:
        return False, f"Pre-tamper frame CRC mismatch: {computed_orig:#x} != {stored_crc:#x}", 0

    corrupt_copy = bytearray(raw)
    tamper_offset = 8 + min(12, max(0, entry_len - 1))
    corrupt_copy[tamper_offset] ^= 0x01
    bad_payload = bytes(corrupt_copy[8 : 8 + entry_len])
    computed_bad = zlib.crc32(bad_payload) & 0xFFFFFFFF

    detected = computed_bad != stored_crc
    detail = (
        f"Byte offset {tamper_offset} flipped (1 bit); "
        f"stored CRC={stored_crc:#010x} vs tampered CRC={computed_bad:#010x}"
    )
    return detected, detail, len(raw)


def save_audit_receipt(receipt: Dict[str, Any]) -> str:
    out_path = os.path.join(SANDBOX_DIR, "security_audit_receipt.json")
    with open(out_path, "w", encoding="utf-8") as f:
        json.dump(receipt, f, indent=2)
    return out_path
