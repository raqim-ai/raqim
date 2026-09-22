import asyncio
import os
import sys
import time
import httpx
from dotenv import load_dotenv

# Path resolution for repository imports
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

# ==============================================================================
# 1. CREDENTIAL PROVISIONING
# ==============================================================================
async def forge_credentials(alias: str, group: str) -> tuple[str, str]:
    key_path = os.path.join(KEY_DIR, f"{alias}.pem")
    cert_path = os.path.join(KEY_DIR, f"{alias}.cert")

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
            json={"agent_hex": agent_hex, "group": group}
        )
        if resp.status_code != 200:
            raise RuntimeError(f"CA Minting failed for {alias}: {resp.text}")
        with open(cert_path, "wb") as f:
            f.write(bytes.fromhex(resp.json()))

    return key_path, cert_path

# ==============================================================================
# 2. PERSISTENT FIREHOSE WORKER
# ==============================================================================
async def run_firehose_worker(agent: RaqimClient, count: int, intent_path: str) -> list[float]:
    """Streams thoughts over a single persistent TCP socket and collects latency."""
    latencies = []
    
    # open_stream maintains 1 dedicated TCP socket connection for the entire batch
    async with agent.open_stream() as stream:
        for i in range(count):
            t0 = time.perf_counter()
            tx_id = await stream(
                intent_path=intent_path,
                text=f"[{agent.alias}] Streaming transaction packet #{i:04d} for compliance audit."
            )
            elapsed_us = (time.perf_counter() - t0) * 1_000_000
            latencies.append(elapsed_us)
            
            if (i + 1) % 50 == 0 or (i + 1) == count:
                avg_lat = sum(latencies[-50:]) / len(latencies[-50:])
                print(f"  ⚡ [{agent.alias}] Committed {i+1}/{count} frames (Recent Avg: {avg_lat:.1f} µs | Last TxID: 0x{tx_id:032x})")
                
    return latencies

# ==============================================================================
# 3. MAIN EXECUTION PIPELINE
# ==============================================================================
async def main():
    print("==================================================================")
    print("Bismillah ar-Rahman ar-Rahim")
    print("Raqim High-Velocity TCP Data Plane: Streaming Firehose Demo")
    print("==================================================================")
    print("Targeting: 127.0.0.1:8080 (TCP Ingress) | 127.0.0.1:8081 (Control Plane)")
    print("👉 Open http://localhost:3000 to watch  the Semantic Firehose update live!\n")

    # Use finance_worker or admin_group to avoid analyst_group's strict 100 TPS quota
    k1, c1 = await forge_credentials("triage_screener", "finance_worker")
    k2, c2 = await forge_credentials("settlement_engine", "finance_worker")

    agent1 = RaqimClient(alias="triage_screener", tenant="production", private_key_path=k1, cert_path=c1)
    agent2 = RaqimClient(alias="settlement_engine", tenant="production", private_key_path=k2, cert_path=c2)

    await agent1.boot()
    await agent2.boot()

    thoughts_per_worker = 150
    print(f"\nLaunching 2 parallel firehose streams ({thoughts_per_worker * 2} thoughts total)...")

    start_time = time.perf_counter()

    # Run both persistent socket streams concurrently
    results = await asyncio.gather(
        run_firehose_worker(agent1, thoughts_per_worker, "/rqm_finance/triage"),
        run_firehose_worker(agent2, thoughts_per_worker, "/rqm_finance/settlement")
    )

    total_duration = time.perf_counter() - start_time
    all_latencies = sorted(results[0] + results[1])
    total_thoughts = len(all_latencies)

    # Calculate real client-observed metrics
    p50 = all_latencies[int(total_thoughts * 0.50)] / 1000
    p95 = all_latencies[int(total_thoughts * 0.95)] / 1000
    p99 = all_latencies[int(total_thoughts * 0.99)] / 1000
    throughput = total_thoughts / total_duration

    print("\n===================================================================")
    print("              RAQIM DATA PLANE BENCHMARK SUMMARY                   ")
    print("===================================================================")
    print(f" Total Thoughts Streamed : {total_thoughts} Frames")
    print(f" Concurrent TCP Sockets  : 2 Dedicated Streams")
    print(f" Total Ingress Duration  : {total_duration:.3f} Seconds")
    print(f" Verified Client TPS     : {throughput:.2f} Thoughts/sec")
    print(f" Median Latency (P50)    : {p50:.3f} ms")
    print(f" Tail Latency   (P95)    : {p95:.3f} ms")
    print(f" Worst Tail     (P99)    : {p99:.3f} ms")
    print("===================================================================")
    print("Alhamdulillah! All frames verified via closed-loop 20-byte server ACKs.")

if __name__ == "__main__":
    asyncio.run(main())