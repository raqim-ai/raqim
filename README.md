# Raqim (رَقِيم)

> **A deterministic cryptographic runtime ledger, flight recorder, and zero-trust security perimeter for autonomous AI agent swarms.**

[![License: Apache-2.0](https://img.shields.io/badge/License-Apache_2.0-blue.svg)](https://opensource.org/licenses/Apache-2.0)
[![CI Release](https://github.com/raqim-ai/raqim/actions/workflows/release.yml/badge.svg)](https://github.com/raqim-ai/raqim/actions)
[![Crates.io](https://img.shields.io/badge/crates.io-v0.1.0-orange)](https://crates.io/crates/raqim-core)
[![PyPI](https://img.shields.io/badge/pypi-v0.1.0-blue)](https://pypi.org/project/raqim/)
[![Throughput](https://img.shields.io/badge/ACK--Throughput-23%2C355_TPS-brightgreen)](#verified-performance-benchmarks)

---

## What Raqim Is

Autonomous AI agents taking actions in production cannot rely on ephemeral prompt-prepending or disposable logs. When an agent fails, loops, or attempts unauthorized operations, engineering and compliance teams require mathematical certainty regarding what occurred.

Raqim provides an immutable execution substrate beneath your agent framework (LangChain, CrewAI, AutoGen, or custom runtimes):

* **Cryptographic Identity (PKI):** Every agent is issued a unique Ed25519 keypair and a signed capability certificate. Unauthenticated or forged packets are dropped at the network edge.
* **Append-Only Durability:** A binary Write-Ahead Log (WAL) with group-commit NVMe synchronization ensures crash-safe durability for state mutations.
* **Domain-Separated Merkle DAG (Axon):** Transitions are batched into 1,024-leaf binary Merkle trees using BLAKE3, generating verifiable offline inclusion proofs.
* **Conflict-Free Convergence:** In-memory CRDT shards (Loro) provide automatic, mathematically conflict-free timeline merging across namespaces.
* **$0 Deterministic Replay & Forking:** The `@client.trace` decorator hashes function signatures with inputs. Replays execute in `<1ms` at **$0 API cost** directly from the WAL. Code divergence automatically isolates state into a parallel universe (`phantom_`) branch.
* **Zero-Trust Firewall (Aegis):** Enforces wildcard namespace access control lists (ACL), atomic token-bucket rate limits, and live quarantine isolation.

---

## Architecture Overview

                    AGENT ENCLAVE (Python / SDK)
                     │
                     │ IngressEnvelope (Ed25519 Signed)
                     ▼
    ┌─────────────────────────────────────────────────┐
    │ RAQIM CORE DAEMON                               │
    │                                                 │
    │ 1. Aegis Security Perimeter                     │
    │    - Master CA Lineage Check                    │
    │    - Fast-Path Token Bucket (CAS Weak Loop)     │
    │    - Namespace ACL & Anti-Replay                │
    │                                                 │
    │ 2. The Ingress Cascade                          │
    │    ├── Loro CRDT Engine (State Convergence)     │
    │    ├── Axon Merkle DAG (BLAKE3 Inclusion Proofs)│
    │    └── Nucleus WAL (2ms NVMe Group Commit)      │
    │                                                 │
    │ 3. Storage & Observability                      │
    │    ├── Hot Vector Buffer (In-RAM Cosine)        │
    │    ├── 2PC Compactor -> LanceDB (Cold Columnar) │
    │    └── Axum HTTP/WS (:8081) -> Next.js Console  │
    └─────────────────────────────────────────────────┘

---

## Verified Performance Benchmarks

All performance claims are reproducible via `raqim-siege`. Throughput figures reflect **Synchronous Closed-Loop Acknowledgment (ACK)**: every measurement represents an end-to-end round trip where the server validated, hashed, sealed, committed, and acknowledged the transaction.

### Test Environment
* **Hardware:** Intel Core i7 (10 Cores / 16 Threads), 16GB RAM, PCIe 4.0 NVMe SSD
* **OS:** Ubuntu 22.04 LTS via WSL2 (Linux 5.15 Kernel)
* **Dataset:** 500,000 thoughts across 50 partitioned agent namespaces
* **Concurrency:** 50 simultaneous non-blocking TCP socket streams

### Latency Distribution & Throughput
| Metric | Measurement | Verification Standard |
| :--- | :--- | :--- |
| **Sustained Throughput** | **23,355.21 TPS** | Closed-loop 20-byte server ACK confirmation |
| **Minimum Latency** | **132 µs (0.132 ms)** | Socket transport + memory validation |
| **P50 Latency (Median)** | **1,853 µs (1.853 ms)** | Aligned to the 2ms physical NVMe group commit interval |
| **P90 Latency** | **3,411 µs (3.411 ms)** | Tail distribution under 50-thread concurrent saturation |
| **P99 Latency (Tail)** | **5,935 µs (5.935 ms)** | Sub-6ms tail latency ceiling |
| **Deterministic Replay** | **< 1.0 ms / $0.00 Cost** | In-memory BLAKE3 signature lookup |
| **Idle Memory (RSS)** | **45 MB** | Process memory footprint post-rehydration |
| **Peak Memory Under Load**| **1,571 MB** | Plateaued resident ceiling across 500k active operations |

---

## Quickstart

### 1. Boot the Daemon
Run via pre-compiled binaries, Docker, or Cargo:

```bash
# Via Docker Compose (Starts Daemon + Admin Console)
docker compose up -d

# Or compile from source
cargo run --release --bin raqim-core
2. Install Python SDKBashpip install raqim
3. Instrument Your First AgentPythonimport asyncio
from raqim import RaqimClient, verify_state_proof_offline

client = RaqimClient(
    alias="security_auditor",
    tenant="production",
    private_key_path="./agent_keys/auditor.pem",
    cert_path="./agent_keys/auditor.cert",
    mode="record"  # Change to "replay" for zero-cost reproduction
)

@client.trace(namespace="/finance/transfers")
def screen_transaction(tx_id: str, amount: float) -> dict:
    return {
        "tx_id": tx_id,
        "amount": amount,
        "flagged": amount >= 9000.0
    }

async def main():
    await client.boot()
    result = screen_transaction("TX_9941", 9950.00)
    print("Audited Result:", result)

if __name__ == "__main__":
    asyncio.run(main())
Security Model (Aegis Perimeter)Raqim implements a Zero-Trust perimeter. Agents do not connect with raw API keys; they present cryptographic capability passports:Ini, TOML# aegis.toml - Hot-Reloadable Security Manifest
[groups.analyst_group]
allowed_namespaces = ["/system/*", "/finance/transfers/*", "/finance/audit/*"]
blocked_namespaces = ["/finance/restricted/*", "/admin/*"]
max_tps = 100
burst_capacity = 200
Lineage Verification: Every certificate is signed by the Swarm Master Key. Forged or expired certificates fail during the initial handshake.Anti-Replay Window: Packets with timestamp drift $> 30\text{s}$ are quarantined automatically.Atomic Rate Limiting: Enforced via Compare-And-Swap (compare_exchange_weak) loops, preventing concurrency underflows.Offline Attestation: Any client or auditor can independently prove state integrity using verify_state_proof_offline() without network calls.LicenseRaqim Core is open-source software licensed under the Apache License 2.0.
---

