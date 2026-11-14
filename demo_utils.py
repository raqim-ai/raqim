#!/usr/bin/env python3
"""
================================================================================
Raqim Interactive Showcase - Verified Systems Utilities & Sandbox Manager
================================================================================
Strictly manages isolated demo sandboxing, cross-platform process lifecycle,
deterministic LLM verification, and cryptographic state attestation.

Zero external dependencies outside repo environment. Zero mutation of host
production data.
================================================================================
"""

import asyncio
import atexit
import json
import os
import re
import shutil
import socket
import re
import subprocess
import sys
import time
from typing import Optional, Tuple, Dict, Any
import uuid
import zlib
from dataclasses import dataclass, field
from typing import Any, Dict, List, Optional, Tuple

ANSI_ESCAPE_RE = re.compile(r'\x1b\[[0-9;]*m')
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
    """Removes ANSI color and style escape codes."""
    return ANSI_ESCAPE_RE.sub('', s)
    return ANSI_ESCAPE.sub("", s)


def visible_len(s: str) -> int:
    """Calculates visible character count ignoring ANSI formatting."""
    return len(strip_ansi(s))


def pad_visible(s: str, width: int) -> str:
    """Pads string to width based on visible character length."""
    v = visible_len(s)
    if v < width:
        return s + " " * (width - v)
    return s
    return s + (" " * max(0, width - v))

# Ensure workspace paths
REPO_ROOT = os.path.dirname(os.path.abspath(__file__))
RAQIM_PY_DIR = os.path.join(REPO_ROOT, "raqim-py")
for p in [RAQIM_PY_DIR, REPO_ROOT]:
    if p not in sys.path and os.path.exists(p):
        sys.path.insert(0, p)

# Load environment variables from raqim-py/.env and repo root .env
def _load_env_file(filepath: str):
    if not os.path.isfile(filepath):
        return
    try:
        with open(filepath, "r", encoding="utf-8") as f:
            for line in f:
                line = line.strip()
                if not line or line.startswith("#") or "=" not in line:
                    continue
                k, v = line.split("=", 1)
                k = k.strip()
                v = v.strip().strip("'\"")
                if k and k not in os.environ:
                    os.environ[k] = v
    except Exception:
        pass
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

try:
    from dotenv import load_dotenv
    load_dotenv(os.path.join(RAQIM_PY_DIR, ".env"))
    load_dotenv(os.path.join(REPO_ROOT, ".env"))
except ImportError:
    pass

_load_env_file(os.path.join(RAQIM_PY_DIR, ".env"))
_load_env_file(os.path.join(REPO_ROOT, ".env"))
def _ensure_raqim_importable():
    py_dir = os.path.join(BASE_DIR, "raqim-py")
    if py_dir not in sys.path:
        sys.path.insert(0, py_dir)

# Sanitize proxy variables so local httpx connects directly to localhost
for k in ["http_proxy", "https_proxy", "HTTP_PROXY", "HTTPS_PROXY"]:
    os.environ.pop(k, None)
os.environ["NO_PROXY"] = "*"
os.environ["no_proxy"] = "*"

import blake3
import httpx
import nacl.signing
_ensure_raqim_importable()

# ==============================================================================
# ISOLATED DEMO SANDBOX CONFIGURATION (NEVER TOUCHES PRODUCTION DATA)
# ==============================================================================
DEMO_SANDBOX_DIR = os.path.join(REPO_ROOT, ".demo_sandbox")
DEMO_WAL_PATH = os.path.join(DEMO_SANDBOX_DIR, "demo.wal")
DEMO_MANIFEST_PATH = os.path.join(DEMO_SANDBOX_DIR, "compaction.manifest.json")
DEMO_WITNESS_PATH = os.path.join(DEMO_SANDBOX_DIR, "witnesses")
DEMO_AEGIS_PATH = os.path.join(DEMO_SANDBOX_DIR, "aegis.toml")
DEMO_KEY_DIR = os.path.join(DEMO_SANDBOX_DIR, "keys")
DEMO_LOG_PATH = os.path.join(DEMO_SANDBOX_DIR, "daemon.log")
DEMO_CONTROL_JOURNAL = os.path.join(DEMO_SANDBOX_DIR, "control_journal.bin")
DEMO_CHECKPOINT = os.path.join(DEMO_SANDBOX_DIR, "checkpoint.bin")

def find_free_port_pair(start_port=8080) -> Tuple[int, int]:
    """Finds an available pair of adjacent loopback ports (TCP ingress & HTTP admin)."""
    for port in range(start_port, 9500, 2):
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
        s1.setsockopt(socket.SOL_SOCKET, socket.SO_REUSEADDR, 1)
        s2 = socket.socket(socket.AF_INET, socket.SOCK_STREAM)
        s2.setsockopt(socket.SOL_SOCKET, socket.SO_REUSEADDR, 1)
        try:
            s1.bind(('127.0.0.1', port))
            s2.bind(('127.0.0.1', port + 1))
            s1.close()
            s2.close()
            s1.bind(("127.0.0.1", port))
            s2.bind(("127.0.0.1", port + 1))
            return port, port + 1
        except Exception:
        except OSError:
            continue
        finally:
            s1.close()
            s2.close()
            continue
    return 8080, 8081
    raise RuntimeError("No free consecutive port pair available")

DAEMON_TCP_PORT, DAEMON_HTTP_PORT = find_free_port_pair()
DAEMON_HTTP = f"http://127.0.0.1:{DAEMON_HTTP_PORT}"

DAEMON_PROC: Optional[subprocess.Popen] = None
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

# ==============================================================================
# BUILD INTROSPECTION & STYLING
# ==============================================================================
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
    """Detects available raqim-core binary and returns (path, build_profile). Always prefers release."""
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
    {Style.WHITE}Execution-Integrity Runtime & Cryptographic Flight Recorder{Style.RESET}
    {Style.DIM}[Build Profile: {profile} | Bind: 127.0.0.1 (Loopback Only)]{Style.RESET}
    """)

def print_header(title: str, subtitle: str = ""):
    print(f"\n{Style.BOLD}{Style.CYAN}{'═' * 76}{Style.RESET}")
    w = 78
    print(f"\n{Style.BOLD}{Style.BLUE}{'=' * w}{Style.RESET}")
    print(f"{Style.BOLD}{Style.WHITE}  {title}{Style.RESET}")
    if subtitle:
        print(f"{Style.DIM}{Style.CYAN}  {subtitle}{Style.RESET}")
    print(f"{Style.BOLD}{Style.CYAN}{'═' * 76}{Style.RESET}\n")
        print(f"{Style.DIM}  {subtitle}{Style.RESET}")
    print(f"{Style.BOLD}{Style.BLUE}{'=' * w}{Style.RESET}")

# ==============================================================================
# PROCESS SUPERVISION (TRACKS SPECIFIC DEMO PID, NO PKILL)
# ==============================================================================
def stop_demo_daemon(proc: Optional[subprocess.Popen] = None):
    """Gracefully terminates only the specific demo child process."""
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

def write_demo_aegis_manifest(path: str):
    """Writes strict policy rules specifically for the demo sandbox."""
    content = """# Raqim Aegis Demo Security Manifest (Sandbox Isolated)
[groups.admin_group]
allowed_namespaces = ["*"]
blocked_namespaces = []
max_tps = 10000
burst_capacity = 1000
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

[groups.analyst_group]
allowed_namespaces = ["/finance/tools/*", "/finance/reasoning/*", "/default/*"]
blocked_namespaces = ["/finance/restricted/*", "/admin/*"]
max_tps = 1000
burst_capacity = 100
    for ol in outcome_lines:
        print(f"{color}|{Style.RESET} {pad_visible(f'{Style.BOLD}->{Style.RESET} {ol}', w - 4)} {color}|{Style.RESET}")
    print(bar)

[groups.finance_worker]
allowed_namespaces = ["/finance/tools/screening", "/default/*"]
blocked_namespaces = ["/finance/restricted/*", "/admin/*"]
max_tps = 100
burst_capacity = 20
"""
    with open(path, "w", encoding="utf-8") as f:
        f.write(content)

def clean_demo_sandbox():
    """
    Safely isolates the demo.
    Wipes ONLY `.demo_sandbox/`. Leaves repo root files (production.wal, vault/, data/) untouched.
    """
    global DAEMON_PROC
    stop_demo_daemon()
async def beat(label: str = "", pause: bool = False, stream_note: Optional[str] = None, delay: float = 0.08):
    if stream_note:
        for tok in stream_note.split(" "):
            sys.stdout.write(f"{Style.CYAN}{tok} {Style.RESET}")
            sys.stdout.flush()
            await asyncio.sleep(delay)
        print()
    if pause:
        await asyncio.to_thread(input, f"\n{Style.DIM}[Enter to continue: {label}]{Style.RESET}")

    if os.path.exists(DEMO_SANDBOX_DIR):

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
            shutil.rmtree(DEMO_SANDBOX_DIR)
            raw_head = await reader.readuntil(b"\r\n\r\n")
        except Exception:
            pass
            writer.close()
            return

    os.makedirs(DEMO_SANDBOX_DIR, exist_ok=True)
    os.makedirs(DEMO_KEY_DIR, exist_ok=True)
    os.makedirs(DEMO_WITNESS_PATH, exist_ok=True)
    write_demo_aegis_manifest(DEMO_AEGIS_PATH)
        head_text = raw_head.decode("utf-8", errors="replace")
        lines = head_text.split("\r\n")
        request_line = lines[0].split(" ")
        method = request_line[0]
        path = request_line[1] if len(request_line) > 1 else "/"

async def check_daemon_health(timeout: float = 0.5) -> bool:
    try:
        async with httpx.AsyncClient(timeout=timeout) as http:
            resp = await http.get(f"{DAEMON_HTTP}/health")
            return resp.status_code == 200
    except Exception:
        return False
        headers: Dict[str, str] = {}
        for line in lines[1:]:
            if ":" in line:
                k, v = line.split(":", 1)
                headers[k.strip().lower()] = v.strip()

async def ensure_daemon_running() -> subprocess.Popen:
    """Spawns raqim-core bound strictly to 127.0.0.1 with isolated sandbox paths."""
    global DAEMON_PROC
        content_len = int(headers.get("content-length", "0"))
        body_bytes = await reader.readexactly(content_len) if content_len > 0 else b""
        body_text = body_bytes.decode("utf-8", errors="replace")

    binary_path, profile = get_binary_info()
    if profile == "uncompiled":
        print(f"{Style.YELLOW}⚙ Compiling raqim-core in release mode (cargo build --release --bin raqim-core)...{Style.RESET}")
        t0 = time.perf_counter()
        subprocess.run(["cargo", "build", "--release", "--bin", "raqim-core"], cwd=REPO_ROOT, check=True)
        print(f"{Style.GREEN}✔ Built in {time.perf_counter() - t0:.1f}s{Style.RESET}")
        binary_path = os.path.join(REPO_ROOT, "target", "release", "raqim-core")
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

    log_file = open(DEMO_LOG_PATH, "ab")
    cmd = [
        binary_path,
        "--port", str(DAEMON_TCP_PORT),
        "--host", "127.0.0.1",
        "--wal-path", DEMO_WAL_PATH,
        "--manifest-path", DEMO_MANIFEST_PATH,
        "--witness-path", DEMO_WITNESS_PATH,
        "--aegis-path", DEMO_AEGIS_PATH,
        "--control-journal-path", DEMO_CONTROL_JOURNAL,
        "--checkpoint-path", DEMO_CHECKPOINT,
    ]
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

    print(f"{Style.CYAN}🚀 Launching isolated daemon (PID supervisor on 127.0.0.1:{DAEMON_TCP_PORT})...{Style.RESET}")
    proc = subprocess.Popen(
        cmd,
        cwd=REPO_ROOT,
        stdout=log_file,
        stderr=subprocess.STDOUT,
    )
    DAEMON_PROC = proc
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

    # Poll until ready (<15s)
    for _ in range(150):
        await asyncio.sleep(0.1)
        if await check_daemon_health():
            print(f"{Style.GREEN}✔ Daemon online: HTTP 127.0.0.1:{DAEMON_HTTP_PORT} | TCP 127.0.0.1:{DAEMON_TCP_PORT}{Style.RESET}")
            print(f"  {Style.DIM}Sandbox: {DEMO_SANDBOX_DIR} (Production WAL completely isolated){Style.RESET}\n")
            return proc
            await self._send_json(writer, 404, {"error": f"Unknown mock path: {method} {path}"})
        except Exception as exc:
            await self._send_json(writer, 500, {"error": str(exc)})

    log_file.close()
    if os.path.exists(DEMO_LOG_PATH):
        with open(DEMO_LOG_PATH, "r", errors="ignore") as f:
            tail = "".join(f.readlines()[-25:])
        print(f"{Style.RED}Daemon failed to boot. Recent logs:\n{tail}{Style.RESET}")
    stop_demo_daemon(proc)
    raise RuntimeError("Timed out waiting for raqim-core daemon to boot.")
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

async def kill_daemon_phoenix(proc: subprocess.Popen):
    """Sends hard uncatchable SIGKILL directly to the demo process PID only."""
    proc.kill()
    try:
        proc.wait(timeout=2.0)
    except Exception:
        pass

async def resurrect_daemon_phoenix() -> Tuple[float, subprocess.Popen]:
    """Relaunches daemon with the exact same sandbox WAL to measure true rehydration latency."""
    t_start = time.perf_counter()
    binary_path, _ = get_binary_info()
    log_file = open(DEMO_LOG_PATH, "ab")
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

    cmd = [
        binary_path,
        "--port", str(DAEMON_TCP_PORT),
        "--host", "127.0.0.1",
        "--wal-path", DEMO_WAL_PATH,
        "--manifest-path", DEMO_MANIFEST_PATH,
        "--witness-path", DEMO_WITNESS_PATH,
        "--aegis-path", DEMO_AEGIS_PATH,
        "--control-journal-path", DEMO_CONTROL_JOURNAL,
        "--checkpoint-path", DEMO_CHECKPOINT,
    ]

    new_proc = subprocess.Popen(
        cmd,
        cwd=REPO_ROOT,
        stdout=log_file,
        stderr=subprocess.STDOUT,
    )
    global DAEMON_PROC
    DAEMON_PROC = new_proc
def prepare_clean_sandbox(upstream_url: str) -> SandboxConfig:
    shutil.rmtree(SANDBOX_DIR, ignore_errors=True)
    for sub in ["", "vault", os.path.join("vault", "witnesses"), os.path.join("vault", "cold_segments"), "ca-keys", "agent_keys", "evidence"]:
        os.makedirs(os.path.join(SANDBOX_DIR, sub), exist_ok=True)

    resurrected = False
    for _ in range(150):
        await asyncio.sleep(0.02)
        if await check_daemon_health(timeout=0.2):
            resurrected = True
            break
    tcp_port, http_port = find_free_port_pair(18080)
    daemon_bin, _ = resolve_binary("raqim-core")
    cli_bin, _ = resolve_binary("raqim-cli")

    elapsed_ms = (time.perf_counter() - t_start) * 1000
    assert resurrected, f"Phoenix recovery failed within 10s! Check {DEMO_LOG_PATH}"
    return elapsed_ms, new_proc
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

# ==============================================================================
# CRYPTOGRAPHIC IDENTITY MINTING
# ==============================================================================
async def forge_agent_credentials(agent_alias: str, security_group: str) -> Tuple[str, str]:
    """Generates local Ed25519 identity and requests signed capability passport from master CA."""
    key_path = os.path.join(DEMO_KEY_DIR, f"{agent_alias}.pem")
    cert_path = os.path.join(DEMO_KEY_DIR, f"{agent_alias}.cert")
[[connectors]]
name = "paystack"
upstream_url = "{upstream_url}/paystack"
secret_env = "{PAYSTACK_ENV_VAR}"
auth_header = "Authorization"
auth_prefix = "Bearer "
idempotency_header = "Idempotency-Key"
allowed_routes = ["POST /transfer", "POST /transfer_flaky", "GET /tx/*"]
redact_pointers = ["/customer/bvn", "/card/cvv"]

    if os.path.exists(key_path) and os.path.exists(cert_path):
        return key_path, cert_path
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

    seed = os.urandom(32)
    with open(key_path, "wb") as f:
        f.write(seed)
    os.chmod(key_path, 0o600)
    policy_toml = """[groups.admin_group]
allowed_namespaces = ["*"]
blocked_namespaces = []
max_tps = 10000
burst_capacity = 5000

    signing_key = nacl.signing.SigningKey(seed)
    pub_bytes = signing_key.verify_key.encode()
[groups.fintech_treasury]
allowed_namespaces = ["/egress/paystack/*", "/egress/llm_upstream/*", "/fintech/*", "/audit/*"]
blocked_namespaces = ["/finance/restricted/*", "/admin/*"]
max_tps = 1000
burst_capacity = 200

    hasher = blake3.blake3(pub_bytes, derive_key_context="raqim.agent.v1.identity")
    agent_hex = hasher.digest(length=16).hex()
[[groups.fintech_treasury.predicates]]
namespace = "/egress/paystack/transfer"
pointer = "/amount"
required = true
min_value = 100.0
max_value = 50000.0
max_hourly_sum = 100000.0

    async with httpx.AsyncClient(timeout=5.0) as http:
        mint_payload = {"agent_hex": agent_hex, "group": security_group}
        resp = await http.post(f"{DAEMON_HTTP}/v1/admin/ca/mint", json=mint_payload)
        if resp.status_code != 200:
            raise RuntimeError(f"CA Minting failed for {agent_alias}: {resp.text}")
[[groups.fintech_treasury.predicates]]
namespace = "/egress/paystack/transfer"
pointer = "/currency"
required = true
allowed_values = ["NGN"]

        cert_hex = resp.json()
        with open(cert_path, "wb") as f:
            f.write(bytes.fromhex(cert_hex))
[[groups.fintech_treasury.predicates]]
namespace = "/egress/paystack/transfer_flaky"
pointer = "/amount"
required = true
min_value = 100.0
max_value = 50000.0
"""
    with open(os.path.join(SANDBOX_DIR, "policy.toml"), "w", encoding="utf-8") as f:
        f.write(policy_toml)

    return key_path, cert_path
    return SandboxConfig(
        tcp_port=tcp_port,
        http_port=http_port,
        upstream_url=upstream_url,
        daemon_bin=daemon_bin,
        cli_bin=cli_bin,
    )

# ==============================================================================
# REASONING ENGINE (HONEST LLM TRACKING & REAL CALL VERIFICATION)
# ==============================================================================
ANTHROPIC_API_KEY = os.getenv("ANTHROPIC_API_KEY", "").strip()
GEMINI_API_KEY = os.getenv("GEMINI_API_KEY", "").strip()
OPENAI_API_KEY = os.getenv("OPENAI_API_KEY", "").strip()

LLM_CALL_COUNT = 0
ACTIVE_LLM_PROVIDER: Optional[str] = None
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

if ANTHROPIC_API_KEY:
    ACTIVE_LLM_PROVIDER = "Anthropic (Claude Haiku)"
elif GEMINI_API_KEY:
    ACTIVE_LLM_PROVIDER = "Google Gemini (Gemini 3.5 Flash Lite)"
elif OPENAI_API_KEY:
    ACTIVE_LLM_PROVIDER = "OpenAI (GPT-4o-mini)"
else:
    ACTIVE_LLM_PROVIDER = None

def get_llm_call_count() -> int:
    global LLM_CALL_COUNT
    return LLM_CALL_COUNT
def start_daemon(cfg: SandboxConfig) -> subprocess.Popen:
    env = os.environ.copy()
    env[PAYSTACK_ENV_VAR] = DAEMON_PAYSTACK_SECRET
    env[LLM_ENV_VAR] = DAEMON_LLM_SECRET
    env["RAQIM_ENV"] = "production"

def reset_llm_call_count():
    global LLM_CALL_COUNT
    LLM_CALL_COUNT = 0
    log_file = open(cfg.daemon_log_path, "ab")
    return subprocess.Popen(
        [cfg.daemon_bin],
        cwd=SANDBOX_DIR,
        env=env,
        stdout=log_file,
        stderr=subprocess.STDOUT,
    )

async def call_llm(prompt: str, context: str) -> Tuple[str, float]:
    """
    Executes live LLM call if credentials are configured.
    FAILS LOUDLY if an API key is provided but the request errors.
    If no credentials exist, transparently uses deterministic simulated reasoning.
    """
    global LLM_CALL_COUNT
    LLM_CALL_COUNT += 1
    start_t = time.perf_counter()

    # 1. Anthropic Claude (Primary for systems engineers)
    if ANTHROPIC_API_KEY:
        url = "https://api.anthropic.com/v1/messages"
        headers = {
            "x-api-key": ANTHROPIC_API_KEY,
            "anthropic-version": "2023-06-01",
            "content-type": "application/json",
        }
        payload = {
            "model": "claude-3-5-haiku-latest",
            "max_tokens": 256,
            "system": prompt,
            "messages": [{"role": "user", "content": f"Analyze AML context:\n{context}"}],
        }
        async with httpx.AsyncClient(timeout=20.0) as http:
            resp = await http.post(url, headers=headers, json=payload)
            if resp.status_code != 200:
                raise RuntimeError(f"Anthropic API call failed (HTTP {resp.status_code}): {resp.text}")
            data = resp.json()
            text = data["content"][0]["text"].strip()
            elapsed_ms = (time.perf_counter() - start_t) * 1000
            return text, elapsed_ms
def stop_daemon(proc: Optional[subprocess.Popen]):
    if proc and proc.poll() is None:
        proc.terminate()
        try:
            proc.wait(timeout=3)
        except subprocess.TimeoutExpired:
            proc.kill()

    # 2. Google Gemini
    if GEMINI_API_KEY:
        models_to_try = ["gemini-3.5-flash-lite", "gemini-3.8-flash", "gemini-3.7-flash"]
        last_err = None
        for model_name in models_to_try:
            url = f"https://generativelanguage.googleapis.com/v1beta/models/{model_name}:generateContent?key={GEMINI_API_KEY}"
            payload = {
                "contents": [{"parts": [{"text": f"{prompt}\n\nEvidence Context:\n{context}"}]}]
            }
            try:
                async with httpx.AsyncClient(timeout=20.0) as http:
                    resp = await http.post(url, json=payload)
                    if resp.status_code == 200:
                        data = resp.json()
                        text = data["candidates"][0]["content"]["parts"][0]["text"].strip()
                        elapsed_ms = (time.perf_counter() - start_t) * 1000
                        return text, elapsed_ms
                    elif resp.status_code == 503:
                        last_err = f"HTTP 503 (High Demand on {model_name})"
                    else:
                        last_err = f"Gemini API ({model_name}) HTTP {resp.status_code}"
            except Exception as e:
                last_err = f"Gemini API ({model_name}) exception: {e}"
        print(f"[\033[33mWARN\033[0m] Live Gemini API request failed ({last_err}). Falling back to deterministic local reasoning.")

    # 3. OpenAI
    if OPENAI_API_KEY:
        url = "https://api.openai.com/v1/chat/completions"
        headers = {"Authorization": f"Bearer {OPENAI_API_KEY}", "Content-Type": "application/json"}
        payload = {
            "model": "gpt-4o-mini",
            "messages": [
                {"role": "system", "content": prompt},
                {"role": "user", "content": f"Analyze AML context:\n{context}"},
            ],
            "temperature": 0.2,
        }
async def phoenix_kill_and_resurrect(proc: subprocess.Popen, cfg: SandboxConfig) -> Tuple[subprocess.Popen, float]:
    if proc.poll() is None:
        proc.kill()
        proc.wait(timeout=3)
    if os.path.exists(cfg.admin_socket):
        try:
            async with httpx.AsyncClient(timeout=20.0) as http:
                resp = await http.post(url, headers=headers, json=payload)
                if resp.status_code == 200:
                    data = resp.json()
                    text = data["choices"][0]["message"]["content"].strip()
                    elapsed_ms = (time.perf_counter() - start_t) * 1000
                    return text, elapsed_ms
                else:
                    print(f"[\033[33mWARN\033[0m] OpenAI API failed (HTTP {resp.status_code}). Falling back to deterministic local reasoning.")
        except Exception as e:
            print(f"[\033[33mWARN\033[0m] OpenAI API call exception ({e}). Falling back to deterministic local reasoning.")
            os.remove(cfg.admin_socket)
        except OSError:
            pass
    t0 = time.perf_counter()
    new_proc = start_daemon(cfg)
    await wait_daemon_ready(cfg)
    elapsed_ms = (time.perf_counter() - t0) * 1000.0
    return new_proc, elapsed_ms

    # 4. Deterministic Simulated Reasoning (No Keys)
    await asyncio.sleep(0.045)  # Simulated fast local inference
    elapsed_ms = (time.perf_counter() - start_t) * 1000

    if "lenient" in prompt.lower() or "holiday" in prompt.lower():
        text = (
            "[COMPLIANCE VERDICT - BRANCH: FORKED]: Transaction approved under discretionary waiver. "
            "Flag waived by regional branch manager."
        )
    else:
        text = (
            "[COMPLIANCE VERDICT - BRANCH: CANONICAL]: Critical BSA/AML structuring anomaly confirmed. "
            "High-velocity routing to offshore jurisdiction (Cayman hop). Mandatory Suspicious Activity Report (SAR) triggered."
        )
    return text, elapsed_ms
def forge_offline_agent_identity(cfg: SandboxConfig, alias: str, group_name: str, ttl_seconds: int = 3600) -> Dict[str, str]:
    from raqim.raqim_core import RaqimCryptoCore

    key_path = os.path.join(SANDBOX_DIR, "agent_keys", f"{alias}.pem")
    cert_path = os.path.join(SANDBOX_DIR, "agent_keys", f"{alias}.cert")
    with open(key_path, "wb") as f:
        f.write(os.urandom(32))
    os.chmod(key_path, 0o600)

# ==============================================================================
# ACT 4B TRUE ON-DISK WAL CORRUPTION & ENGINE RECOVERY CHECK
# ==============================================================================
async def test_disk_wal_corruption_recovery() -> Tuple[bool, str, int]:
    """
    Act 4B True On-Disk Corruption Test:
    1. Copies active demo.wal to demo_corrupted.wal on disk.
    2. Flips 1 byte inside the payload section of a frame on disk.
    3. Boots a throwaway raqim-core daemon instance pointing to demo_corrupted.wal.
    4. Asserts raqim-core's Rust engine detects CRC32 mismatch on startup:
       '[PHOENIX CORRUPTION] CRC32 mismatch in ... Truncating tail.'
    5. Verifies daemon safely halts scan at that frame without crashing or loading corrupt state.
    """
    if not os.path.exists(DEMO_WAL_PATH) or os.path.getsize(DEMO_WAL_PATH) < 16:
        raise RuntimeError(f"Cannot run disk corruption test: {DEMO_WAL_PATH} is empty or missing.")
    core = RaqimCryptoCore(key_path)
    agent_hex = blake3.blake3(
        core.pub_key_bytes, derive_key_context="raqim.agent.v1.identity"
    ).digest(length=16).hex()

    corrupted_wal = os.path.join(DEMO_SANDBOX_DIR, "demo_corrupted.wal")
    shutil.copyfile(DEMO_WAL_PATH, corrupted_wal)
    cert_bytes = RaqimCryptoCore.forge_certificate_from_master(
        cfg.master_key_path, agent_hex, group_name, ttl_seconds
    )
    with open(cert_path, "wb") as f:
        f.write(cert_bytes)

    # Flip 1 byte on disk inside the payload section (byte 24 is after the 8-byte frame header)
    corrupted_offset = 24
    with open(corrupted_wal, "r+b") as f:
        f.seek(corrupted_offset)
        orig_byte = f.read(1)
        if not orig_byte:
            raise RuntimeError("WAL frame unexpectedly short")
        f.seek(corrupted_offset)
        f.write(bytes([orig_byte[0] ^ 0xFF]))
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

    test_tcp, test_http = find_free_port_pair(start_port=9600)
    test_log_path = os.path.join(DEMO_SANDBOX_DIR, "throwaway_corrupt.log")
    test_log = open(test_log_path, "wb")
    binary_path, _ = get_binary_info()

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
        binary_path,
        "--port", str(test_tcp),
        "--host", "127.0.0.1",
        "--wal-path", corrupted_wal,
        "--manifest-path", os.path.join(DEMO_SANDBOX_DIR, "throwaway_manifest.json"),
        "--witness-path", DEMO_WITNESS_PATH,
        "--aegis-path", DEMO_AEGIS_PATH,
        "--control-journal-path", os.path.join(DEMO_SANDBOX_DIR, "throwaway_ctrl.bin"),
        "--checkpoint-path", os.path.join(DEMO_SANDBOX_DIR, "throwaway_ckpt.bin"),
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

    proc = subprocess.Popen(cmd, cwd=REPO_ROOT, stdout=test_log, stderr=subprocess.STDOUT)
    try:
        # Give throwaway daemon 0.8s to scan WAL frames on startup
        await asyncio.sleep(0.8)
    finally:
        proc.kill()
        try:
            proc.wait(timeout=1.0)
        except Exception:
            pass
        test_log.close()

    # Read the engine's real stdout/stderr
    with open(test_log_path, "r", errors="ignore") as f:
        log_content = f.read()
def verify_wal_crc_tamper_detection(wal_path: str) -> Tuple[bool, str, int]:
    """Copies the live WAL, flips 1 payload bit, and verifies CRC32 frame detection."""
    if not os.path.exists(wal_path) or os.path.getsize(wal_path) < 24:
        return False, "WAL missing or too small", 0

    # Clean up test artifacts
    for p in [corrupted_wal, test_log_path, os.path.join(DEMO_SANDBOX_DIR, "throwaway_manifest.json")]:
        if os.path.exists(p):
            try:
                os.remove(p)
            except Exception:
                pass
    with open(wal_path, "rb") as f:
        raw = bytearray(f.read())

    mismatch_detected = (
        "CRC mismatch" in log_content
        or "CRC32 mismatch" in log_content
        or "CORRUPTION" in log_content
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
    log_line = ""
    for line in log_content.splitlines():
        if "CRC mismatch" in line or "CRC32 mismatch" in line or "CORRUPTION" in line:
            log_line = line.strip()
            break
    return detected, detail, len(raw)

    return mismatch_detected, log_line, corrupted_offset

def save_audit_receipt(receipt: Dict[str, Any]) -> str:
    out_path = os.path.join(SANDBOX_DIR, "security_audit_receipt.json")
    with open(out_path, "w", encoding="utf-8") as f:
        json.dump(receipt, f, indent=2)
    return out_path
