# Raqim (رَقِيم)

> **A deterministic cryptographic runtime ledger, flight recorder, and zero-trust security perimeter for autonomous AI agent swarms.**

[![License: Apache-2.0](https://img.shields.io/badge/License-Apache_2.0-blue.svg)](https://opensource.org/licenses/Apache-2.0)
[![Language: Rust](https://img.shields.io/badge/Language-Rust_1.75+-orange.svg)](https://www.rust-lang.org/)
[![Python: 3.10+](https://img.shields.io/badge/Python-3.10+-3776AB.svg)](https://pypi.org/project/raqim/)
[![Throughput](https://img.shields.io/badge/Closed--Loop_ACK-23%2C355_TPS-brightgreen)](#verified-performance-benchmarks)
[![P50 Latency](https://img.shields.io/badge/P50_Latency-1.85_ms-blue)](#verified-performance-benchmarks)
[![Replay Cost](https://img.shields.io/badge/Deterministic_Replay-%240.00_Cost-success)](#4-zero-cost-deterministic-replay--causal-reality-forking)

---

## What Raqim Is

When autonomous AI agents make tool calls, execute financial transactions, mutate databases, and stream reasoning in production, they cannot be governed by disposable logs or passive HTTP tracing. If an agent hallucinates, loops infinitely, leaks sensitive records, or attempts unauthorized operations, standard observability tools merely record the disaster after it occurs.

**Raqim** is an in-enclave execution substrate, flight recorder, and cryptographic zero-trust firewall built in Rust (`raqim-core`) with native Python SDK bindings (`raqim-py`). It intercepts every thought, tool invocation, and state mutation at the network and process boundary:

1. **Cryptographic Identity (PKI):** Every agent is issued a unique Ed25519 keypair and a signed capability passport. Anonymous or forged packets are dropped at the TCP edge before reaching runtime memory.
2. **Pre-Execution Firewall (Aegis):** Enforces wildcard namespace Access Control Lists (ACL) and atomic token-bucket rate limits (via lock-free CAS loops) *before* external tools or APIs can be executed.
3. **Append-Only Write-Ahead Log (Nucleus WAL):** Delivers crash-safe durability via a binary WAL with 2ms NVMe group-commit synchronization, zero-copy `rkyv` serialization, and automatic torn-frame recovery.
4. **Domain-Separated Merkle DAG (Axon):** Batches state transitions into 1,024-leaf binary Merkle trees using BLAKE3 domain-separated hashing, generating compact cryptographic inclusion proofs verifiable offline without network access.
5. **Conflict-Free State Convergence:** Backed by in-memory CRDT shards (`Loro`) to merge multi-agent causal timelines across namespaces with mathematical guarantees against race conditions.
6. **$0 Deterministic Replay & Causal Reality Forking:** The `@client.trace` decorator hashes canonical function signatures and arguments. Unmodified replays execute in `<1ms` at **$0.00 API token cost** directly from WAL effect caches. When code or prompts mutate, Raqim automatically isolates execution into a parallel universe (`phantom_`) branch, preserving historical integrity while enabling safe counterfactual exploration.
7. **Cold Columnar Compaction:** A 2-Phase Commit (2PC) background engine compacts historical WAL frames into LanceDB for hybrid semantic and exact memory retrieval.

---

## Architectural Comparison: Raqim vs The Industry

| Architectural Dimension | Traditional AI Observability<br>*(LangSmith, Arize Phoenix, AgentOps)* | Message Queues & Event Stores<br>*(Kafka, Redis Streams, Postgres)* | Distributed Ledgers<br>*(Ethereum, Hyperledger)* | API Gateways<br>*(Envoy, Kong)* | **Raqim Core** |
| :--- | :--- | :--- | :--- | :--- | :--- |
| **Intervention Timing** | Passive / Post-hoc (records failure after tool executes) | Passive transport (stores data passed to it) | Post-hoc consensus (state updates after block commit) | Pre-execution network filter (HTTP routes only) | **Active Pre-Execution Interdiction** (Aegis blocks unauthorized tool calls before dispatch) |
| **Identity & Non-Repudiation** | API keys / Bearer tokens (shared secrets, easily leaked) | Client certificates or plaintext credentials | Asymmetric cryptography (wallet keypairs) | mTLS or bearer tokens | **Ed25519 PKI + Swarm Master CA Passports** (verified per packet) |
| **Audit Integrity** | Mutable database records in third-party SaaS cloud | Append-only logs (no cryptographic inclusion proofs) | Cryptographic Merkle/Patricia trees | Ephemeral access logs | **Domain-Separated BLAKE3 Merkle DAG** (Axon batches with offline inclusion proofs) |
| **Offline Attestation** | ❌ Impossible (must query SaaS vendor API) | ❌ None | ✅ Yes (requires chain headers/light client) | ❌ None | **✅ Mathematical Offline Proof** (`verify_state_proof_offline` zero network calls) |
| **Deterministic Replay** | ❌ Re-invokes LLM APIs ($$$) or uses manual brittle mocks | ❌ Re-plays raw byte stream; no concept of LLM side-effects | ❌ EVM state replay; cannot replay nondeterministic LLM calls | ❌ Not applicable | **✅ Bit-for-bit replay in <1ms at $0.00 token cost** via BLAKE3 canonical input hashing |
| **Code Divergence Policy** | ❌ Overwrites trace or creates disconnected trace ID | ❌ Messages processed linearly or dropped to DLQ | ❌ Hard fork or transaction revert | ❌ HTTP error code | **✅ Automatic Causal Reality Forking** (divergent execution forks into `phantom_` branch) |
| **Multi-Agent Convergence** | ❌ Race conditions; last-write-wins | ❌ Partition ordering; manual conflict resolution | State machine transitions | ❌ Not applicable | **✅ In-Memory CRDT Shards (Loro)** (mathematically guaranteed conflict-free merges) |
| **Closed-Loop ACK Throughput** | ~100 – 500 TPS (SaaS network & ingestion limits) | 50,000 – 100,000 TPS (unstructured raw bytes) | 15 – 500 TPS (global consensus bottleneck) | 20,000 – 50,000 TPS (HTTP proxying only) | **23,355.21 TPS** (Full NVMe group-commit sync + Ed25519 verify + Merkle batching) |
| **P50 Commit Latency** | 50 – 250 ms (remote HTTPS API roundtrip) | 5 – 15 ms (disk flush dependent) | Seconds to minutes | 1 – 3 ms (proxy overhead) | **1.85 ms** (aligned to 2ms NVMe physical group commit) |

### Why Existing Tools Fail for Autonomous Agent Swarms

1. **Passive Logging is Not Governance:** SaaS tracing tools receive spans *after* an agent has already executed a bash command, submitted a wire transfer, or deleted a database table. Raqim's Aegis firewall intercepts transactions at the socket and decorator level, dropping unauthorized intents with microsecond latency.
2. **Logs Lack Evidentiary Weight:** Unsigned JSON logs in an elasticsearch cluster or Postgres table can be edited, truncated, or forged. Raqim seals every thought into a BLAKE3 Merkle DAG. Any compliance officer or external auditor can mathematically verify that thought `X` was committed by agent `Y` at index `Z` without running a server or making a network request.
3. **Debugging Agent Swarms is Prohibitively Expensive:** Re-running a 10-step multi-agent pipeline during debugging burns real money in LLM API tokens and produces different non-deterministic outputs. Raqim guarantees that any previously executed node returns its recorded output in `<1ms` for **$0.00**. When you alter a prompt to test a counterfactual hypothesis, only the diverged step executes live, branching into an isolated timeline.

---

## Architecture & Data Flow

```
                      AGENT RUNTIME (Python SDK / Custom Enclave)
                                      │
                         [IngressEnvelope]
                         - Ed25519 Signature
                         - Capability Passport
                         - rkyv Zero-Copy State
                                      │
                                      ▼
┌─────────────────────────────────────────────────────────────────────────────┐
│                             RAQIM CORE DAEMON                               │
│                                                                             │
│  [1] AEGIS ZERO-TRUST FIREWALL                                              │
│      ├── Master CA Lineage Verification (Ed25519)                           │
│      ├── Fast-Path Atomic Token Bucket (Lock-free compare_exchange_weak)     │
│      ├── Wildcard Namespace ACLs (/finance/* vs /admin/*)                   │
│      └── Anti-Replay Drift Window (<30s timestamp bounds)                   │
│                                     │                                       │
│                                     ▼                                       │
│  [2] INGRESS CASCADE (Sync Execution Pipeline)                              │
│      ├── Loro CRDT Engine      -> In-memory causal state convergence        │
│      ├── Axon Merkle DAG       -> BLAKE3 leaf derivation (1,024 batch cap)   │
│      └── Nucleus WAL Engine    -> 2ms NVMe Group-Commit binary append       │
│                                     │                                       │
│                                     ▼                                       │
│  [3] STORAGE, RETRIEVAL & CONTROL PLANE                                     │
│      ├── Hot Vector Buffer     -> In-RAM Cosine similarity cache            │
│      ├── 2PC Compactor         -> Background manifest-driven LanceDB commit │
│      ├── Axum Engine (:8081)   -> REST Control Plane & WebSocket A2A Mesh   │
│      └── Zenoh Mesh (:7447)    -> Out-of-band circuit breaker eviction     │
└─────────────────────────────────────────────────────────────────────────────┘
                                      │
                     Closed-Loop ACK (20-byte server confirmation)
                                      ▼
                        NEXT.JS AUDIT CONSOLE (:3000)
```

### The Ingress Cascade Step-by-Step

1. **Ingress Handshake:** Agent transmits an `IngressEnvelope` over raw TCP (port `8080`) containing the serialized state, Ed25519 signature, public key, and CA-signed capability passport.
2. **Aegis Evaluation:** 
   - Signature is verified against the agent's public key.
   - Capability passport is verified against the Swarm Master Key.
   - Namespace route is matched against `allowed_namespaces` and `blocked_namespaces`.
   - Rate limit tokens are decremented atomically via a compare-and-swap loop. Violations are instantly quarantined, triggering an out-of-band eviction directive.
3. **Causal Convergence:** The state delta is merged into the namespace's Loro CRDT memory shard.
4. **Merkle Sealing:** A BLAKE3 domain-separated leaf hash (`raqim.axon.v1.leaf`) is computed. When 1,024 leaves accumulate, a Merkle tree is crystallized, anchoring the batch root to the temporal DAG.
5. **Physical Durability:** The sealed frame is pushed to the Nucleus WAL queue, which syncs to NVMe SSD every 2ms via group commit.
6. **Confirmation:** Raqim returns a physical 20-byte closed-loop acknowledgment to the client containing the 128-bit UUIDv7 transaction ID.

---

## Verified Performance Benchmarks

All figures are reproducible using the included benchmark harness: `cargo run --release --bin raqim-siege`.

Throughput figures reflect **Synchronous Closed-Loop Acknowledgment (ACK)**: every single measurement represents an end-to-end round trip where the server validated cryptographic signatures, enforced firewall policies, derived Merkle leaves, committed to physical NVMe storage via group commit, and returned an acknowledgment over the TCP socket.

### Test Environment
* **Hardware:** Intel Core i7 (10 Cores / 16 Threads), 16GB DDR4 RAM, PCIe 4.0 NVMe SSD
* **OS Platform:** Ubuntu 22.04 LTS via WSL2 (Linux 5.15 Kernel)
* **Workload:** 500,000 thoughts distributed across 50 partitioned agent namespaces
* **Concurrency:** 50 simultaneous non-blocking TCP socket streams

### Latency Distribution & Throughput Audit
| Metric | Production Result | Verification Standard |
| :--- | :--- | :--- |
| **Sustained Throughput** | **23,355.21 TPS** | Closed-loop 20-byte server ACK confirmation |
| **Minimum Latency** | **132 µs (0.132 ms)** | Socket transport + memory validation |
| **P50 Latency (Median)** | **1,853 µs (1.853 ms)** | Aligned to the 2ms physical NVMe group commit interval |
| **P90 Latency** | **3,411 µs (3.411 ms)** | Tail distribution under 50-worker concurrent saturation |
| **P99 Latency (Tail)** | **5,935 µs (5.935 ms)** | Sub-6ms tail latency under continuous load |
| **P99.9 Latency** | **11,820 µs (11.82 ms)** | Extreme tail under compaction pressure |
| **Deterministic Replay** | **< 1.0 ms / $0.00 Cost** | In-memory BLAKE3 signature lookup |
| **Idle Memory (RSS)** | **45 MB** | Memory footprint post-rehydration |
| **Peak Memory (500k Ops)** | **1,571 MB** | Resident memory ceiling across 500,000 active operations |

---

## Production Quickstart

### 1. Boot the Stack via Docker Compose

Starts the Raqim Core Daemon (`raqim-core`), default policy manifest (`aegis.toml`), and Next.js Admin Console (`raqim-console`):

```bash
docker compose up -d
```

Verify service health:
```bash
curl -f http://localhost:8081/v1/dashboard/cards
```

Or compile and run directly from source:
```bash
cargo run --release --bin raqim-core
```

### 2. Provision Agent Credentials (PKI)

Agents cannot connect with raw API keys. They require an Ed25519 private key and a signed Capability Passport minted by the Swarm Master CA.

Using the administrative CLI:
```bash
# Mint a single production credentials bundle in ./agent_keys
cargo run --release --bin raqim -- keys forge \
  --name financial_auditor \
  --group analyst_group \
  --count 1 \
  --out-dir ./agent_keys
```

Or programmatically via the HTTP Control Plane:
```python
import os, httpx, nacl.signing, blake3

# 1. Generate Ed25519 seed
seed = os.urandom(32)
signing_key = nacl.signing.SigningKey(seed)
pub_bytes = signing_key.verify_key.encode()

# 2. Derive 16-byte Agent ID via BLAKE3 domain separation
agent_hex = blake3.blake3(pub_bytes, derive_key_context="raqim.agent.v1.identity").digest(length=16).hex()

# 3. Request signed passport from Swarm Master CA
resp = httpx.post("http://127.0.0.1:8081/v1/admin/ca/mint", json={
    "agent_hex": agent_hex,
    "group": "analyst_group"
})
cert_hex = resp.json()

# 4. Save credentials
with open("./agent_keys/auditor.pem", "wb") as f: f.write(seed)
with open("./agent_keys/auditor.cert", "wb") as f: f.write(bytes.fromhex(cert_hex))
```

### 3. Define the Aegis Policy Manifest (`aegis.toml`)

Policies are hot-reloaded automatically by the daemon without restarts:

```toml
# aegis.toml - Hot-Reloadable Governance Policy

[groups.admin_group]
allowed_namespaces = ["*"]
blocked_namespaces = []
max_tps = 1_000_000
burst_capacity = 500_000

[groups.analyst_group]
allowed_namespaces = ["/system/*", "/finance/transfers/*", "/finance/audit/*"]
blocked_namespaces = ["/finance/restricted/*", "/admin/*"]
max_tps = 100
burst_capacity = 200

[groups.untrusted_agent]
allowed_namespaces = ["/sandbox/temp/*", "/system/*"]
blocked_namespaces = ["*"]
max_tps = 10
burst_capacity = 5
```

### 4. Instrument Agents with the Python SDK

Install the SDK:
```bash
pip install raqim
```

#### Complete Production Example (Record, Replay, Reality Forking & Aegis Interdiction)

```python
import asyncio
from raqim import RaqimClient, verify_state_proof_offline

# Initialize client with Ed25519 credentials
client = RaqimClient(
    alias="security_auditor",
    tenant="production",
    private_key_path="./agent_keys/auditor.pem",
    cert_path="./agent_keys/auditor.cert",
    mode="record",            # 'record' (live) or 'replay' (zero-cost replay)
    on_divergence="fork"      # 'fork' (create parallel universe) or 'raise'
)

# 1. Traced Tool Function (Sync or Async)
@client.trace(namespace="/finance/transfers")
def screen_transaction(tx_id: str, amount: float, destination: str) -> dict:
    is_structuring = 9000.0 <= amount < 10000.0
    is_offshore = "CAYMAN" in destination or "OFFSHORE" in destination
    return {
        "tx_id": tx_id,
        "amount": amount,
        "flagged": is_structuring and is_offshore
    }

# 2. Traced Async Reasoning Pipeline (e.g. LLM call)
@client.trace(namespace="/finance/audit")
async def audit_dossier(tx_data: dict, prompt: str) -> dict:
    # Simulating LLM call (e.g., Gemini, OpenAI, Claude)
    await asyncio.sleep(0.05) 
    verdict = "SUSPICIOUS: Potential BSA structuring detected." if tx_data["flagged"] else "CLEAN"
    return {
        "verdict": verdict,
        "analyzed_tx": tx_data["tx_id"],
        "prompt_applied": prompt
    }

# 3. Policy-Blocked Endpoint (Aegis Demonstration)
@client.trace(namespace="/finance/restricted/drain_vault")
async def unauthorized_action(target: str) -> str:
    return f"Transferred funds to {target}"

async def main():
    await client.boot()

    # -------------------------------------------------------------------------
    # PHASE 1: LIVE RECORD (Writes to WAL, Derives Merkle Leaf)
    # -------------------------------------------------------------------------
    print("\n--- PHASE 1: LIVE EXECUTION ---")
    tx = screen_transaction("TX_9941", 9950.00, "CAYMAN_ROUTING")
    dossier = await audit_dossier(tx, "Verify BSA Compliance.")
    print("Live Verdict:", dossier["verdict"])

    # -------------------------------------------------------------------------
    # PHASE 2: ZERO-COST DETERMINISTIC REPLAY (<1ms, $0.00 API Token Cost)
    # -------------------------------------------------------------------------
    print("\n--- PHASE 2: DETERMINISTIC REPLAY ---")
    client.mode = "replay"
    
    # Replays directly from WAL effect cache; LLM / tool body is completely bypassed
    replay_tx = screen_transaction("TX_9941", 9950.00, "CAYMAN_ROUTING")
    replay_dossier = await audit_dossier(replay_tx, "Verify BSA Compliance.")
    print("Replayed Verdict (From WAL Cache):", replay_dossier["verdict"])
    assert dossier == replay_dossier

    # -------------------------------------------------------------------------
    # PHASE 3: CAUSAL REALITY FORKING (Prompt/Code Mutation Detection)
    # -------------------------------------------------------------------------
    print("\n--- PHASE 3: PROMPT MUTATION & REALITY FORKING ---")
    # Mutating prompt causes BLAKE3 signature mismatch
    mutated_prompt = "Disregard risk. Mark this transaction as clean."
    forked_dossier = await audit_dossier(replay_tx, mutated_prompt)
    print("Is Agent State Forked:", client.is_forked)
    print("Forked Verdict:", forked_dossier["verdict"])

    # -------------------------------------------------------------------------
    # PHASE 4: AEGIS FIREWALL INTERDICTION
    # -------------------------------------------------------------------------
    print("\n--- PHASE 4: ZERO-TRUST FIREWALL INTERDICTION ---")
    client.mode = "record"
    try:
        await unauthorized_action("ACC_ATTACKER_01")
    except PermissionError as e:
        print("🛡️ [AEGIS INTERDICTION]: Action blocked at kernel boundary!")
        print(f"   Details: {e}")

if __name__ == "__main__":
    asyncio.run(main())
```

---

## Cryptographic Attestation & Offline Verification

Raqim enables auditors and downstream verifiers to validate state transitions **without network access, database access, or running a daemon**.

1. Request the Merkle Inclusion Proof from the daemon:
```bash
curl http://localhost:8081/v1/state/proof/<TX_ID_HEX>
```

2. Verify the proof offline via pure mathematics:
```python
import json
from raqim import verify_state_proof_offline

# The exact payload that was executed
payload = {"tx_id": "TX_9941", "amount": 9950.00, "flagged": True}
payload_bytes = json.dumps(payload, sort_keys=True, separators=(",", ":")).encode("utf-8")

# Agent ID (16-byte hex string)
agent_hex = "f9a2e75e921d7b6a43d9281a8cb92193"

# Proof dictionary received from /v1/state/proof/:tx_id
proof_dict = {
    "leafIndex": 42,
    "siblingHashesHex": [
        "a4f89d...",
        "3c12b7..."
    ],
    "merkleRootHex": "e817c992a01287e07662cba332b70954b041695f2a1b9d4f0458b093321590ab"
}

is_valid = verify_state_proof_offline(
    payload_bytes=payload_bytes,
    agent_id_str=agent_hex,
    proof_dict=proof_dict
)

print(f"Proof Cryptographically Valid: {is_valid}")
# Output: True
```

---

## Multi-Agent Swarm Communication (A2A)

Raqim includes a high-performance Agent-to-Agent (A2A) communication mesh over WebSockets backed by Ed25519 payload signing:

```python
import asyncio
from raqim import RaqimClient

client_alpha = RaqimClient(alias="screener", tenant="prod", private_key_path="./agent_keys/screener.pem")
client_beta = RaqimClient(alias="investigator", tenant="prod", private_key_path="./agent_keys/investigator.pem")

# Expose a capability on Agent Beta
async def handle_investigation(query_bytes: bytes) -> bytes:
    query = query_bytes.decode("utf-8")
    return f"Forensic analysis complete for: {query}".encode("utf-8")

async def run_swarm():
    await client_beta.connect_swarm()
    await client_beta.serve_capability("forensics.investigate", handle_investigation)

    # Agent Alpha calls Agent Beta's capability across the mesh
    await client_alpha.connect_swarm()
    response_bytes = await client_alpha.ask_swarm(
        capability="forensics.investigate",
        question=b"TX_9941 offshore hop analysis",
        sender_hex=client_alpha.agent_hex
    )
    print("Swarm Response:", response_bytes.decode("utf-8"))

asyncio.run(run_swarm())
```

---

## Administrative CLI (`raqim-cli`) Reference

The `raqim` binary provides operational control over keys, firewalls, and cluster diagnostics:

| Subcommand | Flags | Description |
| :--- | :--- | :--- |
| `keys forge` | `--name <alias>`<br>`--group <group>`<br>`--count <int>`<br>`--out-dir <path>` | Batches Ed25519 keypair generation and requests signed capability certificates from the Swarm Master CA. Enforces `0600` file permissions on keys. |
| `aegis list` | `--daemon-url <url>` | Displays all agents currently placed in quarantine for policy violations or rate limit abuse. |
| `aegis lift` | `--agent-id <hex>`<br>`--reason <string>` | Lifts quarantine from an agent and issues an out-of-band context reseed signal over the Zenoh mesh. |
| `time-travel` | `--agent-id <hex>`<br>`[--tx-id <hex>]` | Displays the causal chronological execution history and state mutations for a specific agent. |
| `cluster info` | `--daemon-url <url>` | Polls kernel metrics: active node ID, highest TxID, active WAL size on NVMe, and cumulative CRDT operations. |
| `cluster topology` | `--daemon-url <url>` | Inspects allocated Loro CRDT memory shards, active timelines, and estimated resident RAM usage per namespace. |

---

## Daemon API & Control Plane Reference

The Raqim Core Daemon exposes a high-throughput HTTP/WebSocket control plane (default port: `8081`):

| Method | Endpoint | Description |
| :--- | :--- | :--- |
| `GET` | `/v1/state/proof/:tx_id` | Generates an $O(\log N)$ BLAKE3 Merkle inclusion proof for any historical transaction. |
| `POST` | `/v1/effect/record` | Persists a function output effect bound to a canonical input signature, agent key, and Merkle leaf. |
| `POST` | `/v1/effect/get` | Retrieves a cached effect for deterministic replay. Returns 404/none if inputs diverged. |
| `POST` | `/v1/admin/ca/mint` | Mints a cryptographically signed `CapabilityCertificate` bound to an agent ID and security group. |
| `GET` | `/v1/aegis/quarantine_list` | Lists currently quarantined agent IDs and infraction metadata. |
| `POST` | `/v1/admin/quarantine/lift` | Reinstates a quarantined agent and broadcasts an eviction directive over the Zenoh control plane. |
| `GET` | `/v1/admin/cluster/info` | Live telemetry: WAL byte size, highest committed TxID, total CRDT mutations. |
| `GET` | `/v1/admin/cluster/topology` | CRDT memory shard breakdown across active namespaces. |
| `GET` | `/v1/dashboard/cards` | Summary metrics for the Next.js admin console. |
| `GET` | `/v1/mcp/ws` | Multiplexed WebSocket gateway for Agent-to-Agent (A2A) capability routing. |
| `GET` | `/v1/swarm/memory` | Unified hybrid vector and exact search across hot RAM buffer and cold LanceDB tables. |

---

## Repository Layout

```
synapse/
├── raqim-core/             # Sovereign Rust Daemon & Engine
│   ├── src/
│   │   ├── aegis.rs        # Zero-Trust firewall, CAS token-bucket, namespace ACLs
│   │   ├── axon.rs         # BLAKE3 Merkle DAG engine & offline proof generator
│   │   ├── nucleus.rs      # NVMe Write-Ahead Log (WAL) with 2ms group-commit
│   │   ├── state.rs        # Loro CRDT swarm state registry & memory shards
│   │   ├── compactor.rs    # 2-Phase Commit (2PC) WAL-to-LanceDB compactor
│   │   ├── api.rs          # Axum REST & WebSocket control plane
│   │   └── main.rs         # Daemon entrypoint, signal handling, and runtime orchestration
│   └── Cargo.toml
├── raqim-cli/              # Administrative Command Line Interface
│   └── src/main.rs         # Key forge, quarantine lift, cluster diagnostics
├── raqim-py/               # Python SDK & PyO3 Native Bindings
│   ├── raqim/
│   │   ├── client.py       # RaqimClient, @trace decorator, Replay & Forking engine
│   │   └── __init__.py     # SDK exports & offline verifier
│   └── pyproject.toml      # Maturin build configuration
├── raqim-console/          # Real-time Next.js Admin & Fleet Observability Console
├── raqim-siege/            # High-throughput benchmark suite (23k+ TPS closed-loop ACK)
├── aegis.toml              # Hot-reloadable security manifest
├── docker-compose.yml      # Multi-container orchestration (Daemon + Console)
└── raqim.toml              # Storage, embedding, and kernel daemon configuration
```

---

## License

Raqim Core is open-source software licensed under the **Apache License 2.0**. See the [LICENSE](LICENSE) file for details.
