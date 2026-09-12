import asyncio
import os
import sys
import time
import inspect
import httpx
from dotenv import load_dotenv

# ==============================================================================
# 0. DEFENSIVE PATH RESOLUTION & SETUP
# ==============================================================================
CURRENT_DIR = os.path.dirname(os.path.abspath(__file__))
REPO_ROOT = os.path.abspath(os.path.join(CURRENT_DIR, ".."))
RAQIM_PY_DIR = os.path.join(REPO_ROOT, "raqim-py")

for p in [RAQIM_PY_DIR, REPO_ROOT]:
    if p not in sys.path and os.path.exists(p):
        sys.path.insert(0, p)

load_dotenv(os.path.join(REPO_ROOT, ".env"))

from raqim.client import RaqimClient
import blake3
import nacl.signing

DAEMON_HTTP = os.getenv("RAQIM_DAEMON_HTTP", "http://127.0.0.1:8081")
KEY_DIR = os.path.join(REPO_ROOT, "agent_keys")
os.makedirs(KEY_DIR, exist_ok=True)

TOTAL_TRANSACTIONS = 300  # High-velocity batch to showcase live console ingestion

print("==================================================================")
print("Bismillah ar-Rahman ar-Rahim")
print("Raqim High-Velocity TCP Data Plane: Streaming Firehose Demo")
print("==================================================================")
print(f"Targeting: 127.0.0.1:8080 (TCP Ingress) | 127.0.0.1:8081 (Control Plane)")
print("👉 Open http://localhost:3000 to watch the Semantic Firehose render in real time!\n")

# ==============================================================================
# 1. AUTONOMOUS CREDENTIAL PROVISIONING
# ==============================================================================
async def forge_agent_credentials(agent_alias: str, security_group: str) -> tuple[str, str]:
    key_path = os.path.join(KEY_DIR, f"{agent_alias}.pem")
    cert_path = os.path.join(KEY_DIR, f"{agent_alias}.cert")

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
        resp = await http.post(
            f"{DAEMON_HTTP}/v1/admin/ca/mint",
            json={"agent_hex": agent_hex, "group": security_group}
        )
        if resp.status_code != 200:
            raise RuntimeError(f"CA Minting failed for {agent_alias}: {resp.text}")
        with open(cert_path, "wb") as f:
            f.write(bytes.fromhex(resp.json()))

    return key_path, cert_path

# ==============================================================================
# 2. DEFENSIVE DISPATCHER FOR commit_thought
# ==============================================================================
async def dispatch_thought(client: RaqimClient, namespace: str, text: str):
    """Handles 2-parameter or 3-parameter client.commit_thought signatures defensively."""
    sig = inspect.signature(client.commit_thought)
    params = [p for p in sig.parameters if p != "self"]
    if len(params) == 2:
        return await client.commit_thought(namespace, text)
    else:
        return await client.commit_thought(client.agent_hex, namespace, text)

# ==============================================================================
# 3. HIGH-VELOCITY MULTI-AGENT INGRESS PIPELINE
# ==============================================================================
async def stream_agent_firehose(client: RaqimClient, count: int, namespace_prefix: str):
    latencies = []
    
    for i in range(1, count + 1):
        t0 = time.perf_counter()
        
        tx_ref = f"SWIFT-{client.alias[:3].upper()}-{i:04d}"
        amount = 100.0 + (i * 17.5) % 9500.0
        thought_payload = (
            f"TX_EXECUTION: {tx_ref} | Routed ${amount:,.2f} via Liquidity Pool {i % 4} "
            f"| Status: COMMITTED | Monotonic Sequence: #{i}"
        )
        target_ns = f"{namespace_prefix}/batch_{i % 5}"
        
        await dispatch_thought(client, target_ns, thought_payload)
        
        elapsed_us = (time.perf_counter() - t0) * 1_000_000
        latencies.append(elapsed_us)
        
        # Micro-yield every 50 frames to simulate asynchronous agent task loops
        if i % 50 == 0:
            print(f"  ⚡ [{client.alias}] Committed {i}/{count} thoughts to WAL (Avg Latency: {sum(latencies[-50:])/50:.1f} µs)")
            await asyncio.sleep(0.01)

    return latencies

async def main():
    # Step 1: Provision distinct credentials for two concurrent agents
    screener_key, screener_cert = await forge_agent_credentials("triage_worker", "analyst_group")
    settler_key, settler_cert = await forge_agent_credentials("settlement_bot", "admin_group")

    agent_screener = RaqimClient(
        alias="triage_screener",
        tenant="demo_sandbox",
        private_key_path=screener_key,
        cert_path=screener_cert,
    )

    agent_settler = RaqimClient(
        alias="settlement_engine",
        tenant="demo_sandbox",
        private_key_path=settler_key,
        cert_path=settler_cert,
    )

    # Step 2: Open dedicated TCP stream connections to Port 8080
    print("[1/3] Establishing persistent TCP edge connections (Port 8080)...")
    await agent_screener.boot()
    await agent_settler.boot()
    print("  ✅ Both agents connected and handshake validated by microkernel.\n")

    # Step 3: Stream concurrent thought firehose
    print(f"[2/3] Launching parallel firehose ({TOTAL_TRANSACTIONS} thoughts total)...")
    per_worker = TOTAL_TRANSACTIONS // 2
    
    wall_start = time.perf_counter()
    results = await asyncio.gather(
        stream_agent_firehose(agent_screener, per_worker, "/finance/triage"),
        stream_agent_firehose(agent_settler, per_worker, "/finance/settlement"),
    )
    total_duration = time.perf_counter() - wall_start

    all_latencies = results[0] + results[1]
    all_latencies.sort()
    
    total_frames = len(all_latencies)
    effective_tps = total_frames / total_duration
    p50_ms = all_latencies[int(total_frames * 0.50)] / 1000.0
    p95_ms = all_latencies[int(total_frames * 0.95)] / 1000.0
    p99_ms = all_latencies[int(total_frames * 0.99)] / 1000.0

    # Step 4: Verification Summary Report
    print("\n[3/3] =============================================================")
    print("              RAQIM DATA PLANE BENCHMARK SUMMARY                   ")
    print("===================================================================")
    print(f" Total Thoughts Streamed : {total_frames} Frames")
    print(f" Concurrent TCP Sockets  : 2 Dedicated Streams")
    print(f" Total Ingress Duration  : {total_duration:.3f} Seconds")
    print(f" Effective Client TPS    : {effective_tps:,.2f} Thoughts/sec")
    print(f" Median Latency (P50)    : {p50_ms:.3f} ms")
    print(f" Tail Latency   (P95)    : {p95_ms:.3f} ms")
    print(f" Worst Tail     (P99)    : {p99_ms:.3f} ms")
    print("===================================================================")
    print("Alhamdulillah! All thoughts committed to WAL and mirrored to Next.js console.")

if __name__ == "__main__":
    asyncio.run(main())