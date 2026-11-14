#!/usr/bin/env python3
"""
================================================================================
Raqim v0.2.0 Showcase — Isolated Sandbox, Mock Fintech/LLM Upstreams & Admin IPC
================================================================================
- Runs `raqim-core` inside `.demo_sandbox/` so host production files are untouched.
- Injects upstream secrets ONLY into `raqim-core`'s child environment (zero secrets
  exist inside the Python agent process!).
- Communicates with the Operator Admin Plane strictly over `.demo_sandbox/vault/admin.sock`
  with `Authorization: Bearer <admin.secret>`.
================================================================================
"""

import asyncio
import atexit
import json
import os
import re
import shutil
import socket
import subprocess
import sys
import threading
import time
from http.server import BaseHTTPRequestHandler, ThreadingHTTPServer
from typing import Any, Dict, Optional, Tuple

ANSI_ESCAPE_RE = re.compile(r"\x1b\[[0-9;]*m")


def strip_ansi(s: str) -> str:
    return ANSI_ESCAPE_RE.sub("", s)


def visible_len(s: str) -> int:
    return len(strip_ansi(s))


def pad_visible(s: str, width: int) -> str:
    v = visible_len(s)
    return s + " " * (width - v) if v < width else s


REPO_ROOT = os.path.dirname(os.path.abspath(__file__))
RAQIM_PY_DIR = os.path.join(REPO_ROOT, "raqim-py")
for p in [RAQIM_PY_DIR, REPO_ROOT]:
    if p not in sys.path and os.path.exists(p):
        sys.path.insert(0, p)

# Ensure local loopback requests bypass any host proxy
for k in ["http_proxy", "https_proxy", "HTTP_PROXY", "HTTPS_PROXY"]:
    os.environ.pop(k, None)
os.environ["NO_PROXY"] = "*"
os.environ["no_proxy"] = "*"

import blake3
import httpx
from raqim_core import RaqimCryptoCore

# ==============================================================================
# CANARY SECRETS (Injected ONLY into raqim-core child env — NEVER in agent env!)
# ==============================================================================
PAYSTACK_CANARY_SECRET = "sk_live_DEMO_PAYSTACK_CANARY_DO_NOT_LEAK_998877"
OPENAI_CANARY_SECRET = "sk_proj_DEMO_OPENAI_CANARY_DO_NOT_LEAK_776655"

# ==============================================================================
# ISOLATED SANDBOX PATHS
# ==============================================================================
DEMO_SANDBOX_DIR = os.path.join(REPO_ROOT, ".demo_sandbox")
DEMO_WAL_PATH = os.path.join(DEMO_SANDBOX_DIR, "demo.wal")
DEMO_MANIFEST_PATH = os.path.join(DEMO_SANDBOX_DIR, "compaction.manifest.json")
DEMO_WITNESS_PATH = os.path.join(DEMO_SANDBOX_DIR, "vault", "witnesses")
DEMO_COLD_PATH = os.path.join(DEMO_SANDBOX_DIR, "vault", "cold_segments")
DEMO_ADMIN_SOCK = os.path.join(DEMO_SANDBOX_DIR, "vault", "admin.sock")
DEMO_POLICY_PATH = os.path.join(DEMO_SANDBOX_DIR, "policy.toml")
DEMO_CONFIG_PATH = os.path.join(DEMO_SANDBOX_DIR, "raqim.toml")
DEMO_CA_DIR = os.path.join(DEMO_SANDBOX_DIR, "ca-keys")
DEMO_MASTER_KEY = os.path.join(DEMO_CA_DIR, "swarm_master.key")
DEMO_ADMIN_SECRET = os.path.join(DEMO_CA_DIR, "admin.secret")
DEMO_LOG_PATH = os.path.join(DEMO_SANDBOX_DIR, "daemon.log")
DEMO_CONTROL_JOURNAL = os.path.join(DEMO_SANDBOX_DIR, "vault", "control_journal.bin")
DEMO_CHECKPOINT = os.path.join(DEMO_SANDBOX_DIR, "vault", "state_checkpoint.bin")


def find_free_port_pair(start_port: int = 8400) -> Tuple[int, int]:
    for port in range(start_port, 9500, 2):
        s1 = socket.socket(socket.AF_INET, socket.SOCK_STREAM)
        s2 = socket.socket(socket.AF_INET, socket.SOCK_STREAM)
        s1.setsockopt(socket.SOL_SOCKET, socket.SO_REUSEADDR, 1)
        s2.setsockopt(socket.SOL_SOCKET, socket.SO_REUSEADDR, 1)
        try:
            s1.bind(("127.0.0.1", port))
            s2.bind(("127.0.0.1", port + 1))
            s1.close()
            s2.close()
            return port, port + 1
        except Exception:
            s1.close()
            s2.close()
    return 8080, 8081


def find_free_single_port(start_port: int = 9700) -> int:
    for port in range(start_port, 10500):
        s = socket.socket(socket.AF_INET, socket.SOCK_STREAM)
        s.setsockopt(socket.SOL_SOCKET, socket.SO_REUSEADDR, 1)
        try:
            s.bind(("127.0.0.1", port))
            s.close()
            return port
        except Exception:
            s.close()
    return 9700


DAEMON_TCP_PORT, DAEMON_HTTP_PORT = find_free_port_pair()
DAEMON_HTTP = f"http://127.0.0.1:{DAEMON_HTTP_PORT}"
MOCK_UPSTREAM_PORT = find_free_single_port()

DAEMON_PROC: Optional[subprocess.Popen] = None
UPSTREAM_SERVER: Optional[ThreadingHTTPServer] = None


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
    BG_RED = "\033[41m"
    BG_GREEN = "\033[42m"
    BG_BLUE = "\033[44m"


def get_binary_info() -> Tuple[str, str]:
    release_path = os.path.join(REPO_ROOT, "target", "release", "raqim-core")
    debug_path = os.path.join(REPO_ROOT, "target", "debug", "raqim-core")
    if os.path.exists(release_path):
        return release_path, "release (optimized)"
    if os.path.exists(debug_path):
        return debug_path, "debug (unoptimized)"
    return release_path, "uncompiled"


def print_banner():
    _, profile = get_binary_info()
    print(f"""{Style.BOLD}{Style.CYAN}
    ██████╗  █████╗  ██████╗ ██╗███╗   ███╗
    ██╔══██╗██╔══██╗██╔═══██╗██║████╗ ████║
    ██████╔╝███████║██║   ██║██║██╔████╔██║
    ██╔══██╗██╔══██║██║▄▄ ██║██║██║╚██╔╝██║
    ██║  ██║██║  ██║╚██████╔╝██║██║ ╚═╝ ██║
    ╚═╝  ╚═╝╚═╝  ╚═╝ ╚══▀▀═╝ ╚═╝╚═╝     ╚═╝
    {Style.WHITE}Zero-Secret AI Egress Gateway & Execution-Integrity Runtime (v0.2.0){Style.RESET}
    {Style.DIM}[Build Profile: {profile} | Data Plane: 127.0.0.1:{DAEMON_HTTP_PORT} | Admin Plane: UDS 0600]{Style.RESET}
    """)


def print_header(title: str, subtitle: str = ""):
    print(f"\n{Style.BOLD}{Style.CYAN}{'═' * 78}{Style.RESET}")
    print(f"{Style.BOLD}{Style.WHITE}  {title}{Style.RESET}")
    if subtitle:
        print(f"{Style.DIM}{Style.CYAN}  {subtitle}{Style.RESET}")
    print(f"{Style.BOLD}{Style.CYAN}{'═' * 78}{Style.RESET}\n")


# ==============================================================================
# LIVE MOCK UPSTREAM SERVER (PAYSTACK CORE BANKING + OPENAI SSE STREAMING)
# ==============================================================================
UPSTREAM_STATS: Dict[str, Any] = {
    "paystack_hits": 0,
    "openai_hits": 0,
    "last_idempotency_key": "",
    "last_paystack_auth_valid": False,
    "last_paystack_bvn_received": "",
}


def reset_upstream_stats():
    UPSTREAM_STATS["paystack_hits"] = 0
    UPSTREAM_STATS["openai_hits"] = 0
    UPSTREAM_STATS["last_idempotency_key"] = ""
    UPSTREAM_STATS["last_paystack_auth_valid"] = False
    UPSTREAM_STATS["last_paystack_bvn_received"] = ""


class MockFintechAndLlmHandler(BaseHTTPRequestHandler):
    def log_message(self, format: str, *args: Any) -> None:
        pass  # Keep terminal output crisp

    def do_POST(self):
        content_len = int(self.headers.get("Content-Length", "0"))
        raw_body = self.rfile.read(content_len) if content_len > 0 else b"{}"
        auth_hdr = self.headers.get("Authorization", "")
        idem_hdr = self.headers.get("Idempotency-Key", "")

        if self.path.startswith("/transfer"):
            if auth_hdr != f"Bearer {PAYSTACK_CANARY_SECRET}":
                self.send_response(401)
                self.send_header("Content-Type", "application/json")
                self.end_headers()
                self.wfile.write(b'{"status":false,"message":"Invalid Paystack secret key"}')
                return

            body_json = json.loads(raw_body.decode("utf-8"))
            UPSTREAM_STATS["paystack_hits"] += 1
            UPSTREAM_STATS["last_idempotency_key"] = idem_hdr
            UPSTREAM_STATS["last_paystack_auth_valid"] = True
            UPSTREAM_STATS["last_paystack_bvn_received"] = body_json.get("bvn", "")

            resp_obj = {
                "amount": body_json.get("amount"),
                "currency": body_json.get("currency", "NGN"),
                "idempotency_key_echo": idem_hdr,
                "recipient": body_json.get("recipient", "RCP_VERIFIED"),
                "reference": f"PSK_NGN_{UPSTREAM_STATS['paystack_hits']:04d}",
                "status": "TRANSFER_QUEUED",
            }
            resp_bytes = json.dumps(resp_obj, sort_keys=True, separators=(",", ":")).encode("utf-8")
            self.send_response(200)
            self.send_header("Content-Type", "application/json")
            self.send_header("Content-Length", str(len(resp_bytes)))
            self.end_headers()
            self.wfile.write(resp_bytes)
            return

        if self.path.startswith("/v1/chat/completions"):
            if auth_hdr != f"Bearer {OPENAI_CANARY_SECRET}":
                self.send_response(401)
                self.send_header("Content-Type", "application/json")
                self.end_headers()
                self.wfile.write(b'{"error":"Invalid OpenAI API key"}')
                return

            UPSTREAM_STATS["openai_hits"] += 1
            chunks = [
                'data: {"id":"chatcmpl-rqm","choices":[{"delta":{"content":"Treasury "}}]}\n\n',
                'data: {"id":"chatcmpl-rqm","choices":[{"delta":{"content":"audit "}}]}\n\n',
                'data: {"id":"chatcmpl-rqm","choices":[{"delta":{"content":"cleared: "}}]}\n\n',
                'data: {"id":"chatcmpl-rqm","choices":[{"delta":{"content":"25,000 NGN approved."}}]}\n\n',
                "data: [DONE]\n\n",
            ]
            self.send_response(200)
            self.send_header("Content-Type", "text/event-stream")
            self.send_header("Cache-Control", "no-cache")
            self.end_headers()
            for chunk in chunks:
                time.sleep(0.015)  # Simulate real LLM token streaming latency
                self.wfile.write(chunk.encode("utf-8"))
                self.wfile.flush()
            return

        self.send_response(404)
        self.end_headers()


def start_mock_upstream_server():
    global UPSTREAM_SERVER
    if UPSTREAM_SERVER is not None:
        return
    UPSTREAM_SERVER = ThreadingHTTPServer(("127.0.0.1", MOCK_UPSTREAM_PORT), MockFintechAndLlmHandler)
    t = threading.Thread(target=UPSTREAM_SERVER.serve_forever, daemon=True)
    t.start()


def stop_mock_upstream_server():
    global UPSTREAM_SERVER
    if UPSTREAM_SERVER is not None:
        try:
            UPSTREAM_SERVER.shutdown()
            UPSTREAM_SERVER.server_close()
        except Exception:
            pass
        UPSTREAM_SERVER = None


# ==============================================================================
# DAEMON PROCESS SUPERVISION & SANDBOX CONFIG
# ==============================================================================
def stop_demo_daemon(proc: Optional[subprocess.Popen] = None):
    global DAEMON_PROC
    target = proc or DAEMON_PROC
    if target and target.poll() is None:
        target.terminate()
        try:
            target.wait(timeout=2.0)
        except subprocess.TimeoutExpired:
            target.kill()
            try:
                target.wait(timeout=1.0)
            except Exception:
                pass
    if target == DAEMON_PROC:
        DAEMON_PROC = None


atexit.register(stop_demo_daemon)
atexit.register(stop_mock_upstream_server)


def write_demo_configs():
    raqim_toml = f"""# Isolated Raqim v0.2.0 Demo Configuration
tenant_id = "raqim_fintech_airlock_demo"
host = "127.0.0.1"
port = {DAEMON_TCP_PORT}
wal_path = "./demo.wal"
manifest_path = "./compaction.manifest.json"
witness_path = "./vault/witnesses"
aegis_path = "./policy.toml"
checkpoint_path = "./vault/state_checkpoint.bin"
control_journal_path = "./vault/control_journal.bin"
cold_store_path = "./vault/cold_segments"
admin_socket_path = "./vault/admin.sock"

[[connectors]]
name = "paystack"
upstream_url = "http://127.0.0.1:{MOCK_UPSTREAM_PORT}"
secret_env = "DEMO_PAYSTACK_SECRET_KEY"
auth_header = "Authorization"
auth_prefix = "Bearer "
idempotency_header = "Idempotency-Key"
allowed_routes = ["POST /transfer"]
redact_pointers = ["/bvn", "/card/cvv"]

[[connectors]]
name = "openai"
upstream_url = "http://127.0.0.1:{MOCK_UPSTREAM_PORT}"
secret_env = "DEMO_OPENAI_SECRET_KEY"
auth_header = "Authorization"
auth_prefix = "Bearer "
allowed_routes = ["POST /v1/chat/completions"]
redact_pointers = []

[[connectors]]
name = "broken_bank"
upstream_url = "http://127.0.0.1:1"
secret_env = "DEMO_PAYSTACK_SECRET_KEY"
auth_header = "Authorization"
auth_prefix = "Bearer "
idempotency_header = "Idempotency-Key"
allowed_routes = ["POST /wire"]
redact_pointers = []
"""
    with open(DEMO_CONFIG_PATH, "w", encoding="utf-8") as f:
        f.write(raqim_toml)

    policy_toml = """# Raqim v0.2.0 Fintech Airlock Security Policy
[groups.admin_group]
allowed_namespaces = ["*"]
blocked_namespaces = []
max_tps = 10000
burst_capacity = 1000

[groups.treasury_agent]
allowed_namespaces = [
    "/egress/paystack/*",
    "/egress/openai/*",
    "/egress/broken_bank/*",
    "/finance/tools/*",
    "/system/*"
]
blocked_namespaces = ["/finance/restricted/*", "/admin/*"]
max_tps = 200
burst_capacity = 100

[[groups.treasury_agent.predicates]]
namespace = "/egress/paystack/transfer"
pointer = "/amount"
required = true
min_value = 100.0
max_value = 50000.0
max_hourly_sum = 100000.0

[[groups.treasury_agent.predicates]]
namespace = "/egress/paystack/transfer"
pointer = "/currency"
required = true
allowed_values = ["NGN"]
"""
    with open(DEMO_POLICY_PATH, "w", encoding="utf-8") as f:
        f.write(policy_toml)


def clean_demo_sandbox():
    stop_demo_daemon()
    if os.path.exists(DEMO_SANDBOX_DIR):
        shutil.rmtree(DEMO_SANDBOX_DIR, ignore_errors=True)
    os.makedirs(DEMO_CA_DIR, exist_ok=True)
    os.makedirs(DEMO_WITNESS_PATH, exist_ok=True)
    os.makedirs(DEMO_COLD_PATH, exist_ok=True)
    write_demo_configs()
    reset_upstream_stats()
    start_mock_upstream_server()


def _daemon_child_env() -> Dict[str, str]:
    """Secrets are injected ONLY into the Raqim daemon child process, never into Python agent env."""
    env = os.environ.copy()
    env["RAQIM_ENV"] = "production"
    env["DEMO_PAYSTACK_SECRET_KEY"] = PAYSTACK_CANARY_SECRET
    env["DEMO_OPENAI_SECRET_KEY"] = OPENAI_CANARY_SECRET
    return env


async def check_daemon_health(timeout: float = 0.5) -> bool:
    try:
        async with httpx.AsyncClient(timeout=timeout) as http:
            resp = await http.get(f"{DAEMON_HTTP}/health")
            return resp.status_code == 200
    except Exception:
        return False


async def ensure_daemon_running() -> subprocess.Popen:
    global DAEMON_PROC
    binary_path, profile = get_binary_info()
    if profile == "uncompiled":
        subprocess.run(["cargo", "build", "--release", "--bin", "raqim-core"], cwd=REPO_ROOT, check=True)
        binary_path = os.path.join(REPO_ROOT, "target", "release", "raqim-core")

    log_file = open(DEMO_LOG_PATH, "ab")
    proc = subprocess.Popen(
        [binary_path],
        cwd=DEMO_SANDBOX_DIR,
        env=_daemon_child_env(),
        stdout=log_file,
        stderr=subprocess.STDOUT,
    )
    DAEMON_PROC = proc

    for _ in range(100):
        await asyncio.sleep(0.05)
        if await check_daemon_health() and os.path.exists(DEMO_ADMIN_SECRET):
            return proc

    log_file.close()
    raise RuntimeError(f"Timed out waiting for raqim-core to boot. Check {DEMO_LOG_PATH}")


async def kill_daemon_phoenix(proc: subprocess.Popen):
    proc.kill()
    try:
        proc.wait(timeout=2.0)
    except Exception:
        pass


async def resurrect_daemon_phoenix() -> Tuple[float, subprocess.Popen]:
    t_start = time.perf_counter()
    binary_path, _ = get_binary_info()
    log_file = open(DEMO_LOG_PATH, "ab")

    new_proc = subprocess.Popen(
        [binary_path],
        cwd=DEMO_SANDBOX_DIR,
        env=_daemon_child_env(),
        stdout=log_file,
        stderr=subprocess.STDOUT,
    )
    global DAEMON_PROC
    DAEMON_PROC = new_proc

    resurrected = False
    for _ in range(150):
        await asyncio.sleep(0.005)
        if await check_daemon_health(timeout=0.2):
            resurrected = True
            break

    elapsed_ms = (time.perf_counter() - t_start) * 1000
    assert resurrected, f"Phoenix recovery failed! Check {DEMO_LOG_PATH}"
    return elapsed_ms, new_proc


# ==============================================================================
# OFFLINE IDENTITY PROVISIONING & UNIX SOCKET ADMIN IPC
# ==============================================================================
def forge_agent_offline(alias: str, group_name: str) -> Tuple[str, str, str, str]:
    """
    Offline cryptographic provisioning using `.demo_sandbox/ca-keys/swarm_master.key`.
    Returns: (pem_path, cert_path, workload_token, agent_hex).
    """
    pem_path = os.path.join(DEMO_CA_DIR, f"{alias}.pem")
    cert_path = os.path.join(DEMO_CA_DIR, f"{alias}.cert")

    seed = os.urandom(32)
    with open(pem_path, "wb") as f:
        f.write(seed)
    os.chmod(pem_path, 0o600)

    # Derive public key and 16-byte agent_hex via Rust PyO3 core
    tmp_core = RaqimCryptoCore(pem_path, None)
    pub_bytes = bytes(tmp_core.pub_key_bytes)
    agent_hex = blake3.blake3(pub_bytes, derive_key_context="raqim.agent.v1.identity").digest(length=16).hex()

    # Sign binary CapabilityCertificate and `rqm_v1` HTTP Workload Token offline
    cert_bytes = bytes(
        RaqimCryptoCore.forge_certificate_from_master(DEMO_MASTER_KEY, agent_hex, group_name, 86400)
    )
    with open(cert_path, "wb") as f:
        f.write(cert_bytes)

    workload_token = RaqimCryptoCore.forge_workload_token_from_master(
        DEMO_MASTER_KEY, agent_hex, group_name, 86400
    )
    return pem_path, cert_path, workload_token, agent_hex


def read_demo_admin_secret() -> str:
    with open(DEMO_ADMIN_SECRET, "r", encoding="utf-8") as f:
        return f.read().strip()


async def admin_uds_request(method: str, path: str, json_body: Optional[Dict[str, Any]] = None) -> httpx.Response:
    """Communicates exclusively over `.demo_sandbox/vault/admin.sock` with Bearer auth."""
    secret = read_demo_admin_secret()
    transport = httpx.AsyncHTTPTransport(uds=DEMO_ADMIN_SOCK)
    async with httpx.AsyncClient(transport=transport, timeout=5.0) as admin_http:
        return await admin_http.request(
            method.upper(),
            f"http://localhost{path}",
            headers={"Authorization": f"Bearer {secret}"},
            json=json_body,
        )


async def test_mid_file_wal_corruption_halt() -> Tuple[bool, str, int]:
    """
    Copies `demo.wal` to a throwaway file, flips 1 byte inside an earlier batch frame,
    and launches a throwaway daemon instance to verify that Raqim detects mid-file
    CRC32 corruption and REFUSES to silently truncate subsequent data.
    """
    corrupt_wal = os.path.join(DEMO_SANDBOX_DIR, "corrupt_audit.wal")
    shutil.copyfile(DEMO_WAL_PATH, corrupt_wal)

    flip_offset = 16
    with open(corrupt_wal, "r+b") as f:
        f.seek(flip_offset)
        orig = f.read(1)
        f.seek(flip_offset)
        f.write(bytes([orig[0] ^ 0xFF]))

    binary_path, _ = get_binary_info()
    throwaway_tcp, _ = find_free_port_pair(start_port=9502)
    throwaway_log = os.path.join(DEMO_SANDBOX_DIR, "corrupt_check.log")
    throwaway_sock = os.path.join(DEMO_SANDBOX_DIR, "vault", "throwaway_admin.sock")

    cmd = [
        binary_path,
        "--port", str(throwaway_tcp),
        "--host", "127.0.0.1",
        "--wal-path", corrupt_wal,
        "--admin-socket-path", throwaway_sock,
        "--control-journal-path", os.path.join(DEMO_SANDBOX_DIR, "vault", "throwaway_cj.bin"),
        "--checkpoint-path", os.path.join(DEMO_SANDBOX_DIR, "vault", "throwaway_ckpt.bin"),
    ]

    with open(throwaway_log, "wb") as lf:
        proc = subprocess.Popen(cmd, cwd=DEMO_SANDBOX_DIR, env=_daemon_child_env(), stdout=lf, stderr=subprocess.STDOUT)
        await asyncio.sleep(0.5)
        if proc.poll() is None:
            proc.kill()
            proc.wait(timeout=1.0)

    with open(throwaway_log, "r", errors="ignore") as f:
        log_text = f.read()

    for p in [corrupt_wal, throwaway_log, throwaway_sock]:
        if os.path.exists(p):
            try:
                os.remove(p)
            except Exception:
                pass

    detected = ("CRC mismatch" in log_text) or ("MidFileCorruption" in log_text) or ("CORRUPTION" in log_text)
    matched_line = ""
    for line in log_text.splitlines():
        if "CRC mismatch" in line or "MidFileCorruption" in line or "CORRUPTION" in line:
            matched_line = line.strip()
            break
    return detected, matched_line, flip_offset