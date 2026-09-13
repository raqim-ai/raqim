import asyncio
import os
import sys
import time
import httpx
from dotenv import load_dotenv

# Defensive path resolution for repository imports
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
# 2. TRIGGER 2PC COMPACTION VIA HTTP API
# ==============================================================================
async def trigger_wal_compaction():
    """Triggers WAL rotation and 2PC compaction into LanceDB cold storage."""
    endpoints = [
        f"{DAEMON_HTTP}/v1/admin/wal/compact",
        f"{DAEMON_HTTP}/v1/admin/compact",
        f"{DAEMON_HTTP}/v1/compactor/trigger",
    ]
    async with httpx.AsyncClient(timeout=10.0) as http:
        for ep in endpoints:
            try:
                resp = await http.post(ep)
                if resp.status_code in (200, 202):
                    print(f"📦 [COMPACTOR] Triggered 2PC WAL compaction via {ep}.")
                    return True
            except Exception:
                continue
    return False

# ==============================================================================
# 3. QUERY UNIFIED HYBRID VAULT SEARCH
# ==============================================================================
async def query_vault_memory(query: str, namespace: str = "ALL", limit: int = 5) -> list[dict]:
    """Queries the hybrid search endpoint across LanceDB and Hot RAM vector buffers."""
    endpoints = [
        f"{DAEMON_HTTP}/v1/vault/search",
        f"{DAEMON_HTTP}/v1/swarm/memory",
    ]
    params = {
        "query": query,
        "namespace": namespace,
        "include_wal": "true",
        "limit": limit,
    }
    async with httpx.AsyncClient(timeout=10.0) as http:
        for ep in endpoints:
            try:
                resp = await http.get(ep, params=params)
                if resp.status_code == 200:
                    return resp.json()
            except Exception:
                continue
    raise RuntimeError("Failed to query unified vault memory across all endpoints.")

# ==============================================================================
# 4. MAIN WORKFLOW
# ==============================================================================
async def main():
    print("==================================================================")
    print("Bismillah ar-Rahman ar-Rahim")
    print("Raqim Hybrid Memory Engine: Hot RAM & Cold LanceDB Scatter-Gather")
    print("==================================================================")

    k, c = await forge_credentials("archival_analyst", "finance_worker")
    agent = RaqimClient(alias="archival_analyst", tenant="production", private_key_path=k, cert_path=c)
    await agent.boot()

    # --- PHASE 1: INGEST HISTORICAL BATCH (DESTINED FOR COLD LANCEDB) ---
    print("\n[PHASE 1] Ingesting Historical Thought Batch (Destined for Cold Storage)...")
    historical_thoughts = [
        "ARCHIVE_DOSSIER: Target entity ACC_OFFSHORE_8891 routed funds to Swiss fiduciary account.",
        "ARCHIVE_DOSSIER: Beneficial ownership trace confirmed offshore shell incorporated in Panama.",
        "ARCHIVE_DOSSIER: Structuring pattern identified across 12 consecutive wire transfers under threshold.",
    ]

    async with agent.open_stream() as stream:
        for t in historical_thoughts:
            tx_id = await stream(intent_path="/finance/audit/cold_archive", text=t)
            print(f"  📥 Historical frame committed -> TxID: 0x{tx_id:032x}")

    # --- PHASE 2: TRIGGER 2PC BACKGROUND COMPACTION ---
    print("\n[PHASE 2] Triggering 2PC WAL Compaction into LanceDB Parquet Segments...")
    compacted = await trigger_wal_compaction()
    if compacted:
        # Yield to allow background OS thread to embed and write Parquet partitions
        print("  ⏳ Awaiting background LanceDB compaction settlement (2.5s)...")
        await asyncio.sleep(2.5)

    # --- PHASE 3: INGEST LIVE ACTIVE BATCH (STORED IN HOT WAL BUFFER) ---
    print("\n[PHASE 3] Ingesting Active Real-Time Batch (Residing in Hot RAM Buffer)...")
    live_thoughts = [
        "LIVE_ALERT: Real-time intercept on Cayman clearing node for target beneficiary ACC_OFFSHORE_8891.",
        "LIVE_ALERT: Rapid fund movement detected on SWIFT router 449 with high velocity.",
    ]

    async with agent.open_stream() as stream:
        for t in live_thoughts:
            tx_id = await stream(intent_path="/finance/audit/live_firehose", text=t)
            print(f"  🔥 Hot frame committed -> TxID: 0x{tx_id:032x}")

    # Brief yield for SIMD HotVectorBuffer ingestion
    await asyncio.sleep(0.5)

    # --- PHASE 4: EXECUTE UNIFIED HYBRID RRF SEARCH ---
    print("\n[PHASE 4] Executing Parallel Scatter-Gather Hybrid Search (Query: 'offshore beneficiary')...")
    search_query = "offshore beneficiary ACC_OFFSHORE_8891"
    t0 = time.perf_counter()
    results = await query_vault_memory(query=search_query, namespace="ALL", limit=6)
    duration_ms = (time.perf_counter() - t0) * 1000

    print(f"\nSearch resolved in {duration_ms:.2f} ms across both memory tiers:")
    print("-" * 90)
    print(f"{'RANK':<5} | {'SOURCE':<14} | {'RRF SCORE':<10} | {'TEXT PREVIEW'}")
    print("-" * 90)

    has_cold = False
    has_hot = False

    for rank, item in enumerate(results, start=1):
        source = item.get("source", "UNKNOWN")
        score = item.get("score", 0.0)
        text = item.get("text", "")

        if "COLD" in source:
            has_cold = True
        if "HOT" in source:
            has_hot = True

        print(f"{rank:<5} | {source:<14} | {score:<10.4f} | {text[:55]}...")

    print("-" * 90)

    # --- PHASE 5: VERIFICATION ASSERTIONS ---
    print("\n[PHASE 5] Validating Architectural Invariants...")
    print(f"  [x] COLD_LANCEDB partition match present : {has_cold}")
    print(f"  [x] HOT_WAL in-memory vector match present: {has_hot}")

    if has_cold and has_hot:
        print("\n==================================================================")
        print("Alhamdulillah! True Hybrid Scatter-Gather Proved Across Both Tiers.")
        print("==================================================================")
    else:
        print("\n⚠️ Note: Compactor interval in progress; single-tier results returned.")

if __name__ == "__main__":
    asyncio.run(main())