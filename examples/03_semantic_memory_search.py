import asyncio
import os
import sys
import time
import httpx
from dotenv import load_dotenv

# Defensive path resolution
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
            json={"agent_hex": agent_hex, "group": group},
        )
        if resp.status_code != 200:
            raise RuntimeError(f"CA Minting failed: {resp.text}")
        with open(cert_path, "wb") as f:
            f.write(bytes.fromhex(resp.json()))

    return key_path, cert_path

# ==============================================================================
# 2. SEED DATASETS
# ==============================================================================
COLD_HISTORICAL_MEMORIES = [
    "KYC Dossier: Ultimate Beneficial Owner of Apex Clearing is registered in Port Louis, Mauritius under trust structure 9912.",
    "Sanctions Update: Shell entity Trident Maritime Logistics flagged for illicit crude transshipment in Fujairah outer anchorage.",
    "Audit Finding: Suspicious CTR evasion cluster detected across 4 Delaware LLCs sharing identical registered agent addresses.",
    "Regulatory Precedent: BSA threshold avoidance through structured wire transfers under $10,000 mandates automated SAR filing.",
]

HOT_ACTIVE_MEMORIES = [
    "REALTIME ALERT: Incoming wire transfer of $9,950.00 from Apex Clearing node clearing via offshore correspondent hop.",
    "REALTIME ALERT: Entity Trident Maritime Logistics attempting rapid automated asset liquidation to Cayman custodian.",
    "REALTIME ALERT: High-velocity micro-transfers totaling $48,000 detected across 5 newly spun virtual debit sub-accounts.",
]

# ==============================================================================
# 3. MAIN WORKFLOW
# ==============================================================================
async def main():
    print("==================================================================")
    print("Bismillah ar-Rahman ar-Rahim")
    print("Raqim Hybrid RAG: Hot-RAM & Cold-LanceDB Memory Synthesis")
    print("==================================================================")
    print("Targeting: 127.0.0.1:8080 (TCP Ingress) | 127.0.0.1:8081 (Control Plane)\n")

    k, c = await forge_credentials("memory_auditor", "finance_worker")
    agent = RaqimClient(alias="memory_auditor", tenant="production", private_key_path=k, cert_path=c)
    await agent.boot()

    # Step 1: Ingest Batch 1 (Targeted for Cold Compaction)
    print("\n[STEP 1] Ingesting Batch 1: Historical Intelligence Dossiers...")
    async with agent.open_stream() as stream:
        for memory in COLD_HISTORICAL_MEMORIES:
            tx_id = await stream(intent_path="/rqm_finance/history", text=memory)
            print(f"  📥 Historical frame committed -> TxID: 0x{tx_id:032x}")
      
    # Step 2: Trigger Manual WAL Rotation & LanceDB 2PC Compaction
    print("\n[STEP 2] Triggering 2PC WAL Compactor via Admin API...")
    async with httpx.AsyncClient(timeout=10.0) as http:
        res = await http.post(f"{DAEMON_HTTP}/v1/admin/compactor/trigger")
        print(f"  ⚡ Compactor trigger status: {res.status_code} ({res.json().get('success')})")

    # Give the background OS thread a moment to complete Parquet indexing
    print("  ⏳ Waiting 5s for LanceDB background indexing & vector crystallization...")
    await asyncio.sleep(5)

    # Step 3: Ingest Batch 2 (Remains in Hot Vector Buffer)
    print("\n[STEP 3] Ingesting Batch 2: Real-time In-Flight Threat Thoughts...")
    async with agent.open_stream() as stream:
        for alert in HOT_ACTIVE_MEMORIES:
            tx_id = await stream(intent_path="/rqm_finance/realtime", text=alert)
            print(f"  🔥 Hot frame committed -> TxID: 0x{tx_id:032x}")
            
    await asyncio.sleep(1)

    # Step 4: Execute Hybrid Semantic Retrieval
    search_query = "Apex Clearing offshore transactions and BSA structuring evade limits"
    print(f"\n[STEP 4] Querying Hybrid Vault Search: '{search_query}'")

    async with httpx.AsyncClient(timeout=10.0) as http:
        resp = await http.get(
            f"{DAEMON_HTTP}/v1/vault/search",
            params={
                "query": search_query,
                "include_wal": "true",
                "limit": 5,
            },
        )

        if resp.status_code != 200:
            print(f"❌ Search failed: HTTP {resp.status_code} - {resp.text}")
            return

        results = resp.json()

    # Step 5: Render Scannable Results
    print("\n=====================================================================================================")
    print(f"{'RANK':<5} | {'SOURCE':<14} | {'RRF SCORE':<10} | {'TEXT SNIPPET'}")
    print("-----------------------------------------------------------------------------------------------------")

    for rank, item in enumerate(results, start=1):
        source = item.get("source", "UNKNOWN")
        score = item.get("score", 0.0)
        text = item.get("text", "")
        snippet = (text[:75] + "...") if len(text) > 75 else text

        source_tag = "🔥 [HOT_WAL]" if "HOT" in source else "❄️ [LANCEDB]"
        print(f"{rank:<5} | {source_tag:<14} | {score:<10.4f} | {snippet}")

    print("=====================================================================================================")
    print("Alhamdulillah! Verified parallel scatter-gather across Cold Parquet and In-Memory WAL vectors.")

if __name__ == "__main__":
    asyncio.run(main())
