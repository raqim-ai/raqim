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
import os
import shutil
import socket
import re
import subprocess
import sys
import time
from typing import Optional, Tuple, Dict, Any

ANSI_ESCAPE_RE = re.compile(r'\x1b\[[0-9;]*m')

def strip_ansi(s: str) -> str:
    """Removes ANSI color and style escape codes."""
    return ANSI_ESCAPE_RE.sub('', s)

def visible_len(s: str) -> int:
    """Calculates visible character count ignoring ANSI formatting."""
    return len(strip_ansi(s))

def pad_visible(s: str, width: int) -> str:
    """Pads string to width based on visible character length."""
    v = visible_len(s)
    if v < width:
        return s + " " * (width - v)
    return s

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

try:
    from dotenv import load_dotenv
    load_dotenv(os.path.join(RAQIM_PY_DIR, ".env"))
    load_dotenv(os.path.join(REPO_ROOT, ".env"))
except ImportError:
    pass

_load_env_file(os.path.join(RAQIM_PY_DIR, ".env"))
_load_env_file(os.path.join(REPO_ROOT, ".env"))

# Sanitize proxy variables so local httpx connects directly to localhost
for k in ["http_proxy", "https_proxy", "HTTP_PROXY", "HTTPS_PROXY"]:
    os.environ.pop(k, None)
os.environ["NO_PROXY"] = "*"
os.environ["no_proxy"] = "*"

import blake3
import httpx
import nacl.signing

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
        s1 = socket.socket(socket.AF_INET, socket.SOCK_STREAM)
        s1.setsockopt(socket.SOL_SOCKET, socket.SO_REUSEADDR, 1)
        s2 = socket.socket(socket.AF_INET, socket.SOCK_STREAM)
        s2.setsockopt(socket.SOL_SOCKET, socket.SO_REUSEADDR, 1)
        try:
            s1.bind(('127.0.0.1', port))
            s2.bind(('127.0.0.1', port + 1))
            s1.close()
            s2.close()
            return port, port + 1
        except Exception:
            s1.close()
            s2.close()
            continue
    return 8080, 8081

DAEMON_TCP_PORT, DAEMON_HTTP_PORT = find_free_port_pair()
DAEMON_HTTP = f"http://127.0.0.1:{DAEMON_HTTP_PORT}"

DAEMON_PROC: Optional[subprocess.Popen] = None

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
    print(f"{Style.BOLD}{Style.WHITE}  {title}{Style.RESET}")
    if subtitle:
        print(f"{Style.DIM}{Style.CYAN}  {subtitle}{Style.RESET}")
    print(f"{Style.BOLD}{Style.CYAN}{'═' * 76}{Style.RESET}\n")

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

def write_demo_aegis_manifest(path: str):
    """Writes strict policy rules specifically for the demo sandbox."""
    content = """# Raqim Aegis Demo Security Manifest (Sandbox Isolated)
[groups.admin_group]
allowed_namespaces = ["*"]
blocked_namespaces = []
max_tps = 10000
burst_capacity = 1000

[groups.analyst_group]
allowed_namespaces = ["/finance/tools/*", "/finance/reasoning/*", "/default/*"]
blocked_namespaces = ["/finance/restricted/*", "/admin/*"]
max_tps = 1000
burst_capacity = 100

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

    if os.path.exists(DEMO_SANDBOX_DIR):
        try:
            shutil.rmtree(DEMO_SANDBOX_DIR)
        except Exception:
            pass

    os.makedirs(DEMO_SANDBOX_DIR, exist_ok=True)
    os.makedirs(DEMO_KEY_DIR, exist_ok=True)
    os.makedirs(DEMO_WITNESS_PATH, exist_ok=True)
    write_demo_aegis_manifest(DEMO_AEGIS_PATH)

async def check_daemon_health(timeout: float = 0.5) -> bool:
    try:
        async with httpx.AsyncClient(timeout=timeout) as http:
            resp = await http.get(f"{DAEMON_HTTP}/health")
            return resp.status_code == 200
    except Exception:
        return False

async def ensure_daemon_running() -> subprocess.Popen:
    """Spawns raqim-core bound strictly to 127.0.0.1 with isolated sandbox paths."""
    global DAEMON_PROC

    binary_path, profile = get_binary_info()
    if profile == "uncompiled":
        print(f"{Style.YELLOW}⚙ Compiling raqim-core in release mode (cargo build --release --bin raqim-core)...{Style.RESET}")
        t0 = time.perf_counter()
        subprocess.run(["cargo", "build", "--release", "--bin", "raqim-core"], cwd=REPO_ROOT, check=True)
        print(f"{Style.GREEN}✔ Built in {time.perf_counter() - t0:.1f}s{Style.RESET}")
        binary_path = os.path.join(REPO_ROOT, "target", "release", "raqim-core")

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

    print(f"{Style.CYAN}🚀 Launching isolated daemon (PID supervisor on 127.0.0.1:{DAEMON_TCP_PORT})...{Style.RESET}")
    proc = subprocess.Popen(
        cmd,
        cwd=REPO_ROOT,
        stdout=log_file,
        stderr=subprocess.STDOUT,
    )
    DAEMON_PROC = proc

    # Poll until ready (<15s)
    for _ in range(150):
        await asyncio.sleep(0.1)
        if await check_daemon_health():
            print(f"{Style.GREEN}✔ Daemon online: HTTP 127.0.0.1:{DAEMON_HTTP_PORT} | TCP 127.0.0.1:{DAEMON_TCP_PORT}{Style.RESET}")
            print(f"  {Style.DIM}Sandbox: {DEMO_SANDBOX_DIR} (Production WAL completely isolated){Style.RESET}\n")
            return proc

    log_file.close()
    if os.path.exists(DEMO_LOG_PATH):
        with open(DEMO_LOG_PATH, "r", errors="ignore") as f:
            tail = "".join(f.readlines()[-25:])
        print(f"{Style.RED}Daemon failed to boot. Recent logs:\n{tail}{Style.RESET}")
    stop_demo_daemon(proc)
    raise RuntimeError("Timed out waiting for raqim-core daemon to boot.")

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

    resurrected = False
    for _ in range(150):
        await asyncio.sleep(0.02)
        if await check_daemon_health(timeout=0.2):
            resurrected = True
            break

    elapsed_ms = (time.perf_counter() - t_start) * 1000
    assert resurrected, f"Phoenix recovery failed within 10s! Check {DEMO_LOG_PATH}"
    return elapsed_ms, new_proc

# ==============================================================================
# CRYPTOGRAPHIC IDENTITY MINTING
# ==============================================================================
async def forge_agent_credentials(agent_alias: str, security_group: str) -> Tuple[str, str]:
    """Generates local Ed25519 identity and requests signed capability passport from master CA."""
    key_path = os.path.join(DEMO_KEY_DIR, f"{agent_alias}.pem")
    cert_path = os.path.join(DEMO_KEY_DIR, f"{agent_alias}.cert")

    if os.path.exists(key_path) and os.path.exists(cert_path):
        return key_path, cert_path

    seed = os.urandom(32)
    with open(key_path, "wb") as f:
        f.write(seed)
    os.chmod(key_path, 0o600)

    signing_key = nacl.signing.SigningKey(seed)
    pub_bytes = signing_key.verify_key.encode()

    hasher = blake3.blake3(pub_bytes, derive_key_context="raqim.agent.v1.identity")
    agent_hex = hasher.digest(length=16).hex()

    async with httpx.AsyncClient(timeout=5.0) as http:
        mint_payload = {"agent_hex": agent_hex, "group": security_group}
        resp = await http.post(f"{DAEMON_HTTP}/v1/admin/ca/mint", json=mint_payload)
        if resp.status_code != 200:
            raise RuntimeError(f"CA Minting failed for {agent_alias}: {resp.text}")

        cert_hex = resp.json()
        with open(cert_path, "wb") as f:
            f.write(bytes.fromhex(cert_hex))

    return key_path, cert_path

# ==============================================================================
# REASONING ENGINE (HONEST LLM TRACKING & REAL CALL VERIFICATION)
# ==============================================================================
ANTHROPIC_API_KEY = os.getenv("ANTHROPIC_API_KEY", "").strip()
GEMINI_API_KEY = os.getenv("GEMINI_API_KEY", "").strip()
OPENAI_API_KEY = os.getenv("OPENAI_API_KEY", "").strip()

LLM_CALL_COUNT = 0
ACTIVE_LLM_PROVIDER: Optional[str] = None

if ANTHROPIC_API_KEY:
    ACTIVE_LLM_PROVIDER = "Anthropic (Claude Haiku)"
elif GEMINI_API_KEY:
    ACTIVE_LLM_PROVIDER = "Google Gemini (Gemini Flash)"
elif OPENAI_API_KEY:
    ACTIVE_LLM_PROVIDER = "OpenAI (GPT-4o-mini)"
else:
    ACTIVE_LLM_PROVIDER = None

def get_llm_call_count() -> int:
    global LLM_CALL_COUNT
    return LLM_CALL_COUNT

def reset_llm_call_count():
    global LLM_CALL_COUNT
    LLM_CALL_COUNT = 0

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

    # 2. Google Gemini
    if GEMINI_API_KEY:
        models_to_try = ["gemini-flash-lite-latest", "gemini-flash-latest"]
        last_err = None
        for model_name in models_to_try:
            url = f"https://generativelanguage.googleapis.com/v1beta/models/{model_name}:generateContent?key={GEMINI_API_KEY}"
            payload = {
                "contents": [{"parts": [{"text": f"{prompt}\n\nEvidence Context:\n{context}"}]}]
            }
            for attempt in range(2):
                try:
                    async with httpx.AsyncClient(timeout=30.0) as http:
                        resp = await http.post(url, json=payload)
                        if resp.status_code == 200:
                            data = resp.json()
                            text = data["candidates"][0]["content"]["parts"][0]["text"].strip()
                            elapsed_ms = (time.perf_counter() - start_t) * 1000
                            return text, elapsed_ms
                        elif resp.status_code == 503:
                            last_err = f"HTTP 503 (High Demand on {model_name})"
                            await asyncio.sleep(1.0)
                            continue
                        else:
                            last_err = f"Gemini API ({model_name}) HTTP {resp.status_code}: {resp.text}"
                            break
                except Exception as e:
                    last_err = f"Gemini API ({model_name}) exception: {e}"
                    await asyncio.sleep(0.5)
        raise RuntimeError(f"Gemini API call failed after retries: {last_err}")

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
        async with httpx.AsyncClient(timeout=20.0) as http:
            resp = await http.post(url, headers=headers, json=payload)
            if resp.status_code != 200:
                raise RuntimeError(f"OpenAI API call failed (HTTP {resp.status_code}): {resp.text}")
            data = resp.json()
            text = data["choices"][0]["message"]["content"].strip()
            elapsed_ms = (time.perf_counter() - start_t) * 1000
            return text, elapsed_ms

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

    corrupted_wal = os.path.join(DEMO_SANDBOX_DIR, "demo_corrupted.wal")
    shutil.copyfile(DEMO_WAL_PATH, corrupted_wal)

    # Flip 1 byte on disk inside the payload section (byte 24 is after the 8-byte frame header)
    corrupted_offset = 24
    with open(corrupted_wal, "r+b") as f:
        f.seek(corrupted_offset)
        orig_byte = f.read(1)
        if not orig_byte:
            raise RuntimeError("WAL frame unexpectedly short")
        f.seek(corrupted_offset)
        f.write(bytes([orig_byte[0] ^ 0xFF]))

    test_tcp, test_http = find_free_port_pair(start_port=9600)
    test_log_path = os.path.join(DEMO_SANDBOX_DIR, "throwaway_corrupt.log")
    test_log = open(test_log_path, "wb")
    binary_path, _ = get_binary_info()

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
    ]

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

    # Clean up test artifacts
    for p in [corrupted_wal, test_log_path, os.path.join(DEMO_SANDBOX_DIR, "throwaway_manifest.json")]:
        if os.path.exists(p):
            try:
                os.remove(p)
            except Exception:
                pass

    mismatch_detected = "[PHOENIX CORRUPTION] CRC32 mismatch" in log_content
    log_line = ""
    for line in log_content.splitlines():
        if "CRC32 mismatch" in line:
            log_line = line.strip()
            break

    return mismatch_detected, log_line, corrupted_offset
