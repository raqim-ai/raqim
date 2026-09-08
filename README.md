<div align="center">

# RAQIM (رَقِيم)

**The Sovereign Agent Operating System, Cryptographic Flight Recorder & Zero-Trust Governance Kernel for Enterprise AI Swarms**

*Named after Ar-Raqim (الرقيم) — the ancient inscribed ledger referenced in Surah Al-Kahf (18:9).*

[![License: Apache-2.0](https://img.shields.io/badge/License-Apache_2.0-blue.svg)](https://opensource.org/licenses/Apache-2.0)
[![Rust: 2021](https://img.shields.io/badge/Rust-1.75+-orange.svg)](https://www.rust-lang.org)
[![Python: 3.10+](https://img.shields.io/badge/Python-3.10+-3776AB.svg?logo=python&logoColor=white)](https://pypi.org/project/raqim/)
[![Model Context Protocol](https://img.shields.io/badge/MCP-Compatible-purple.svg)](https://modelcontextprotocol.io)
[![Closed-Loop ACK](https://img.shields.io/badge/Closed--Loop_ACK-23%2C355_TPS-brightgreen.svg)](#verified-performance-benchmarks)
[![Peak Wire Ingress](https://img.shields.io/badge/Peak_Ingress-105%2C240_TPS-success.svg)](#verified-performance-benchmarks)
[![Deterministic Replay](https://img.shields.io/badge/Replay_Cost-%240.00_%2F_%3C1ms-blueviolet.svg)](#zero-cost-deterministic-replay)
[![BLAKE3 Merkle DAG](https://img.shields.io/badge/Integrity-BLAKE3_Merkle--DAG-critical.svg)](#mathematical-inclusion-proofs)

---

</div>

## Table of Contents

- [What Raqim Is](#what-raqim-is)
- [The Production Crisis: Why Swarms Need a Kernel](#the-production-crisis-why-swarms-need-a-kernel)
- [Paradigm Shift: Traditional Stacks vs. Raqim AOS](#paradigm-shift-traditional-stacks-vs-raqim-aos)
- [System Architecture & Ingress Cascade](#system-architecture--ingress-cascade)
- [Core Subsystems](#core-subsystems)
- [Verified Performance Benchmarks](#verified-performance-benchmarks)
- [Quickstart & Deployment](#quickstart--deployment)
  - [Option 1: Docker Compose](#option-1-docker-compose-recommended)
  - [Option 2: Native Cargo Compilation](#option-2-native-cargo-compilation)
- [Python SDK & Runtime Guide](#python-sdk--runtime-guide-raqim-py)
  - [Production Tracing (Record Mode)](#1-production-tracing-record-mode)
  - [Zero-Cost Deterministic Replay](#2-zero-cost-deterministic-replay-0-cost--03ms)
  - [Causal Reality Forking](#3-causal-reality-forking-what-if-branching)
  - [Zero-Trust Offline Attestation](#4-offline-cryptographic-attestation)
- [Model Context Protocol Gateway (raqim-mcp)](#model-context-protocol-gateway-raqim-mcp)
- [Fleet Administration CLI (raqim-cli)](#fleet-administration-cli-raqim-cli)
- [Zero-Trust Security Manifest (aegis.toml)](#zero-trust-security-manifest-aegistoml)
- [Mission Control Dashboard (raqim-console)](#mission-control-dashboard-raqim-console)
- [Monorepo Subsystems](#monorepo-subsystems)
- [Enterprise Compliance & Forensic Auditability](#enterprise-compliance--forensic-auditability)
- [License](#license)

---

## What Raqim Is

Autonomous AI agents taking actions in enterprise production cannot rely on ephemeral prompt-prepending, disposable logs, or volatile memory. When an autonomous agent hallucinates, loops destructively, triggers unauthorized financial transfers, or causes data corruption, engineering and compliance teams are left debugging stochastic black boxes — burning thousands of dollars in duplicate LLM token bills without reproducible proof of past states.

**Raqim** is an ultra-high-performance microkernel built from the ground up in Rust that serves as a **Sovereign Agent Operating System (AOS)** and **Cryptographic Flight Recorder**. Every thought, tool invocation, CRDT mutation, and external network effect is cryptographically bound into an append-only Write-Ahead Log (WAL), sealed within an in-memory BLAKE3 Merkle-DAG, and verified under a zero-trust capability firewall.

With Raqim, autonomous AI swarms achieve:
- **Sub-microsecond execution recording** via zero-copy `rkyv` binary frames.
- **Hardware-synced durability** via 2ms NVMe group commit flushes.
- **$0-cost deterministic historical replay** served from memory in under 1ms.
- **Causal reality forking** into isolated `phantom_` branches upon logic mutation.
- **Mathematical inclusion proofs** verifiable completely offline without network dependencies.
- **Zero-trust perimeter security** enforced through Ed25519 PKI capability passports.

---

## The Production Crisis: Why Swarms Need a Kernel

Modern agent frameworks (LangChain, CrewAI, AutoGen, OpenAI Swarm) orchestrate non-deterministic reasoning over volatile infrastructure. This creates critical operational liabilities in production:

1. **The Hallucination Tax:** Reproducing an edge-case bug requires re-invoking non-deterministic LLMs. You pay full token costs again, and because sampling is stochastic, you often fail to reproduce the defect.
2. **The Mutable Evidence Problem:** Standard database rows and application logs can be truncated, manipulated, or silently dropped. They provide zero mathematical or cryptographic proof for compliance audits (SOC2, HIPAA, FinCEN).
3. **Multi-Agent State Corruption:** Uncoordinated agents reading and writing to shared memory cause race conditions, split-brain realities, and lock contention.
4. **Unchecked Tool Agency:** Giving an LLM direct API access without a kernel-level security monitor leaves systems vulnerable to prompt injections, SSRF, and unintended destructive operations.

Raqim solves these systemic flaws by acting as the **immutable substrate** underneath your agent framework.

---

## Paradigm Shift: Traditional Stacks vs. Raqim AOS

| Operational Dimension | Traditional Frameworks (LangChain, CrewAI, AutoGen) | Raqim Agent Operating System (AOS) |
|---|---|---|
| **State Persistence** | Ephemeral RAM, Redis caches, or mutable SQL tables | Zero-copy binary WAL with NVMe group-commit sync & CRC32 integrity |
| **Incident Debugging** | Re-run stochastic LLMs at full token cost | Deterministic replay from disk in `<1ms` at **$0.00 API token cost** |
| **What-If Exploration** | Destructive overwrites or manual state duplicating | **Causal Reality Forking**: Automatic isolation into `phantom_` CRDT branches |
| **Consensus & Memory** | Race conditions, lock contention, split-brain states | **Loro CRDTs**: Conflict-free, lockless causal timeline sharding |
| **Forensic Auditability** | Plaintext logs easily altered or deleted | **Axon Merkle-DAG**: Signed BLAKE3 inclusion proofs + WORM `chattr +i` immutability |
| **Network Wire Ingress** | 500 – 2,000 req/sec over JSON/HTTP | **105,240+ TPS** raw TCP over realigned zero-copy binary frames |
| **Durability Throughput** | Synchronous DB writes stall at 500–1,500 TPS | **23,355+ TPS** closed-loop ACK with physical NVMe group commit |
| **Tool Execution Safety** | Blind trust in system prompts and API keys | **Aegis Firewall**: Ed25519 capability passports, CAS rate limiting, instant quarantine |
| **IDE & Desktop Control** | Ad-hoc Python scripts and custom bridges | **Native MCP Gateway**: Stdio bridge for Claude Desktop, Cursor, and Windsurf |

---

## System Architecture & Ingress Cascade

```
                          SWARM AGENT INGRESS
    ┌─────────────────────────────────────────────────────────────┐
    │  Raw TCP Stream (:8080)   │   MCP Stdio (raqim-mcp)         │
    │  Axum Control (:8081)     │   Zenoh P2P Gossip (:7447)      │
    └──────────────────────────────┬──────────────────────────────┘
                                   │ Length-Prefixed rkyv Envelopes
                                   ▼
    ┌─────────────────────────────────────────────────────────────┐
    │                   AEGIS SECURITY FIREWALL                   │
    │  • Ed25519 Master CA Lineage Verification                   │
    │  • Anti-Replay Drift Window (< 30s Drift Enforcement)       │
    │  • Atomic Token-Bucket CAS Loop (Compare-And-Swap)          │
    │  • Wildcard Namespace Access Control Lists (ACL)            │
    │  • Distributed Quarantine State Mesh                        │
    └──────────────────────────────┬──────────────────────────────┘
                                   │ Authorized Envelopes
                                   ▼
    ┌─────────────────────────────────────────────────────────────┐
    │                  execute_raqim_cascade()                    │
    │  • Monotonic UUIDv7 Generation                              │
    │  • In-Memory Frame Realignment (AlignedVec)                 │
    └──────────────┬───────────────┼───────────────┬──────────────┘
                   │               │               │
        ┌──────────┴──────┐ ┌──────┴─────────┐ ┌───┴──────────────┐
        ▼                 ▼ ▼                ▼ ▼                  ▼
 ┌─────────────┐   ┌─────────────┐   ┌─────────────┐   ┌──────────────┐
 │ NUCLEUS WAL │   │    AXON     │   │  LORO CRDT  │   │  HYBRID RAG  │
 │ Sequential  │   │ Merkle DAG  │   │ Conflict-   │   │ In-RAM Ring  │
 │ NVMe Group  │   │ 1,024-Leaf  │   │ Free Swarm  │   │ Buffer (10k) │
 │ Commit (2ms)│   │ BLAKE3 Root │   │ Sharding    │   │ LanceDB Cold │
 │ CRC32 Check │   │ Proof Gen   │   │ Lockless    │   │ RRF + Decay  │
 └──────┬──────┘   └──────┬──────┘   └──────┬──────┘   └──────┬───────┘
        │                 │                 │                 │
        ▼                 ▼                 ▼                 ▼
 ┌─────────────┐   ┌─────────────┐   ┌─────────────┐   ┌──────────────┐
 │2PC COMPACTOR│   │WORM WITNESS │   │ ZENOH MESH  │   │ AXUM CONTROL │
 │ Hot WAL ->  │   │Ed25519 Sign │   │ P2P Swarm   │   │ HTTP / WS /  │
 │ LanceDB     │   │Linux +i WORM│   │ Gossip A2A  │   │ SSE Stream   │
 └─────────────┘   └─────────────┘   └─────────────┘   └──────────────┘
```

---

## Core Subsystems

### 1. Nucleus Write-Ahead Log (WAL)
Nucleus provides bare-metal durability. Ingress envelopes are serialized using zero-copy `rkyv`, framed with a 4-byte length prefix and a 4-byte CRC32 integrity checksum, and batched into a memory-aligned buffer. A dedicated background flusher coordinates hardware-synchronized NVMe group-commits on a 2ms interval. On cold boot, `recover_and_truncate_torn_frames` performs complete forensic hydration, repairing torn writes and establishing a sparse offset index in under 1.5 seconds across 500,000 transactions.

### 2. Axon Merkle DAG
Every state transition is hashed using domain-separated BLAKE3 contexts (`raqim.axon.v1.leaf` and `raqim.axon.v1.node`). Axon batches thoughts into binary Merkle trees of exactly 1,024 leaves. Once full, the tree crystallizes: its Merkle root is calculated, signed by the node, and chained to previous batch roots. Any agent or external auditor can extract an inclusion proof consisting of $\log_2(1024) = 10$ sibling hashes to mathematically prove that an action occurred at an exact microsecond without network access.

### 3. Loro CRDT Engine
To prevent lock contention across distributed swarms, state is partitioned into conflict-free replicated data types via Loro CRDTs. Agents emit causal state mutations that merge deterministically across namespaces. No locks, no database transactions, and no race conditions.

### 4. Aegis Zero-Trust Security Perimeter
Aegis enforces edge admission before packets touch kernel memory:
- **Lineage Verification:** Every agent must present a cryptographic capability passport signed by the Swarm Master Key.
- **Anti-Replay Window:** Packets with timestamp drift exceeding 30 seconds are rejected and immediately flagged.
- **Atomic Token-Bucket (CAS):** Rate limits are enforced via lock-free `compare_exchange_weak` atomic loops, preventing concurrency underflow vulnerabilities.
- **Wildcard Namespace ACLs:** Strict path-based permission enforcement (e.g., `/finance/tools/*` vs. `/finance/restricted/*`).
- **Live Quarantine Isolation:** Compromised or misbehaving agents are instantly evicted into quarantine, broadcasting eviction notices across the Zenoh mesh.

### 5. Dual-Tier Hybrid Memory Router
Context search spans two high-performance tiers:
- **Hot Tier:** In-RAM 10,000-slot vector ring buffer computing real-time cosine similarity over recent thoughts.
- **Cold Tier:** Columnar vector indexing backed by LanceDB for petabyte-scale historical recall.
- **Fusion:** Results are merged using Reciprocal Rank Fusion (RRF) with continuous exponential temporal decay, prioritizing fresh memories by 25%.

### 6. Two-Phase Commit (2PC) Compactor & WORM Witness
To prevent unbounded WAL disk growth, the 2PC Compactor sweeps aged, crystallized batches into LanceDB columnar storage. The Witness Engine then signs batch manifests and anchors them into Write-Once-Read-Many (WORM) storage using the Linux kernel immutable attribute (`chattr +i`), creating legal-grade tamper resistance even against compromised `root` accounts.

---

## Verified Performance Benchmarks

All metrics are benchmarked using the included `raqim-siege` harness under continuous saturation. Raqim demonstrates dual performance excellence: wire-speed zero-copy network ingress and guaranteed physical NVMe persistence.

### Test Environment Specification
- **Host CPU:** Intel Core i7 (10 Cores / 16 Threads) @ 5.4 GHz
- **Storage:** PCIe Gen4 NVMe M.2 SSD (7,000 MB/s sequential write)
- **Memory:** 16 GB DDR5 RAM
- **Operating System:** Ubuntu 22.04 LTS via WSL2 (Linux 5.15 Kernel)
- **Workload:** 500,000 agent thoughts across 50 partitioned agent shards
- **Concurrency:** 50 simultaneous non-blocking TCP socket streams

### Verified Ingress & Durability Results

| Metric | Measurement | Architecture Mechanism | Verification Standard |
|---|---|---|---|
| **Peak Wire Ingress** | **105,240 TPS** | Zero-copy `rkyv` deserialization + realigned vectors | Raw TCP socket firehose |
| **Closed-Loop ACK Throughput** | **23,355.21 TPS** | Full round-trip: validated, hashed, WAL synced, acknowledged | 20-byte server ACK confirmation |
| **Minimum Latency** | **132 µs (0.132 ms)** | Direct memory dispatch + socket transport | Wire-to-ACK turnaround |
| **P50 Median Latency** | **1,853 µs (1.853 ms)** | Aligned to the physical 2ms NVMe group commit interval | Saturated multi-tenant load |
| **P90 Latency** | **3,411 µs (3.411 ms)** | Tail distribution across 50 concurrent sockets | Saturated multi-tenant load |
| **P99 Tail Latency** | **5,935 µs (5.935 ms)** | Lock-free CAS token bucket & zero-alloc ring buffers | High-load tail latency ceiling |
| **Deterministic Replay** | **< 1.0 ms / $0.00** | BLAKE3 signature hit from memory-mapped cache | Zero external API calls |
| **Crash Recovery (Hydration)** | **500k logs in < 1.5s** | CRC32-validated binary WAL stream scan | Clean reboot state rehydration |
| **Resident Memory (Idle)** | **45 MB RSS** | Lean Rust microkernel footprint post-boot | Base system baseline |
| **Resident Memory (Peak 500k)**| **1,571 MB RSS** | Bounded in-RAM ring buffers + Loro CRDT sharding | Under maximum continuous saturation |

### Reproduce Benchmarks Locally
Run the siege harness against a local running daemon:

```bash
cargo run --release -p raqim-siege
```

---

## Quickstart & Deployment

### Option 1: Docker Compose (Recommended)

Deploy the full stack — including the Raqim Core Daemon and Next.js Mission Control Console:

```yaml
# docker-compose.yml
version: "3.8"

services:
  raqim-daemon:
    image: ghcr.io/raqim-ai/raqim/core:latest
    container_name: raqim-core-daemon
    restart: unless-stopped
    ports:
      - "8080:8080"   # Raw TCP Zero-Copy Ingress Firehose
      - "8081:8081"   # HTTP/WS Control Plane and SSE Stream
      - "7447:7447"   # Zenoh P2P Swarm Network Mesh
    volumes:
      - ./data:/var/lib/raqim/data
      - ./ca-keys:/var/lib/raqim/ca-keys
      - ./vault:/var/lib/raqim/vault
      - ./aegis.toml:/var/lib/raqim/aegis.toml
    environment:
      - RUST_LOG=info
      - RAQIM_PORT=8080
    healthcheck:
      test: ["CMD-SHELL", "curl -f http://localhost:8081/v1/dashboard/cards || exit 1"]
      interval: 10s
      timeout: 5s
      retries: 3

  raqim-console:
    image: ghcr.io/raqim-ai/raqim/console:latest
    container_name: raqim-console-ui
    restart: unless-stopped
    ports:
      - "3000:3000"   # Next.js Management UI
    environment:
      - NEXT_PUBLIC_RAQIM_DAEMON_URL=http://localhost:8081
      - RAQIM_INTERNAL_DAEMON_URL=http://raqim-daemon:8081
    depends_on:
      raqim-daemon:
        condition: service_healthy
```

Start the cluster:

```bash
docker compose up -d
```

Verify daemon health:

```bash
curl -s http://localhost:8081/v1/dashboard/cards | jq .
```

---

### Option 2: Native Cargo Compilation

Compile and launch the high-performance daemon directly from source:

```bash
# Clone the repository
git clone https://github.com/raqim-ai/raqim.git
cd raqim/synapse

# Run the core daemon in release mode
cargo run --release --bin raqim-core
```

---

## Python SDK & Runtime Guide (raqim-py)

Install the official client:

```bash
pip install raqim httpx
```

### 1. Production Tracing (Record Mode)

Decorate agent tools and LLM chains with `@client.trace`. Raqim cryptographically signs each invocation, records input/output signatures into the WAL, and seals state into the Merkle-DAG:

```python
import asyncio
import os
from raqim import RaqimClient

# Initialize sovereign agent identity with capability credentials
client = RaqimClient(
    alias="fraud_auditor",
    tenant="production",
    private_key_path="./agent_keys/auditor.pem",
    cert_path="./agent_keys/auditor.cert",
    mode="record",           # Record mode: streams live execution to WAL
    on_divergence="fork",     # Automatically fork universe if logic diverges
)

# Trace tool execution into Raqim's Merkle-DAG
@client.trace(namespace="/finance/tools/screening")
def screen_transaction(tx_id: str, amount: float, destination: str) -> dict:
    is_structuring = (9000.0 <= amount < 10000.0)
    is_high_risk = "OFFSHORE" in destination
    return {
        "tx_id": tx_id,
        "amount": amount,
        "flagged": is_structuring and is_high_risk,
    }

# Trace LLM chain step
@client.trace(namespace="/finance/reasoning/audit")
async def generate_verdict(evidence: dict) -> dict:
    # Live LLM call (e.g. Gemini, OpenAI, Claude, or local Ollama)
    verdict = f"Flagged account transfer of ${evidence['amount']:,.2f} for compliance review."
    return {"verdict": verdict, "status": "ESCALATED"}

async def main():
    await client.boot()
    
    # Live execution: committed to WAL, sealed into Merkle-DAG
    evidence = screen_transaction("TX_9941", 9950.00, "CAYMAN_ISLANDS")
    result = await generate_verdict(evidence)
    print("Live Record Result:", result)

if __name__ == "__main__":
    asyncio.run(main())
```

---

### 2. Zero-Cost Deterministic Replay ($0 Cost / < 0.3ms)

Switch `mode="replay"`. Raqim computes the domain-separated BLAKE3 hash of the function signature and input arguments (`raqim.effect.v1.signature`). It intercepts the call and serves the recorded output directly from disk cache in under 0.3 milliseconds — **$0 API token cost, zero network latency**:

```python
# Replay configuration: 100% deterministic reproduction
client = RaqimClient(
    alias="fraud_auditor",
    tenant="production",
    private_key_path="./agent_keys/auditor.pem",
    cert_path="./agent_keys/auditor.cert",
    mode="replay",  # Hits WAL cache; zero LLM token consumption
)

async def main():
    await client.boot()
    
    # Executes in 0.2ms with $0.00 token cost
    evidence = screen_transaction("TX_9941", 9950.00, "CAYMAN_ISLANDS")
    result = await generate_verdict(evidence)
    print("Replayed Result (Zero Cost):", result)

if __name__ == "__main__":
    asyncio.run(main())
```

---

### 3. Causal Reality Forking (What-If Branching)

When prompts, model weights, or tool logic mutate during a replay run, Raqim detects the execution divergence. Rather than crashing or polluting primary production state, the kernel automatically isolates the mutated timeline into a `phantom_` CRDT namespace branch:

```python
client = RaqimClient(
    alias="fraud_auditor",
    tenant="production",
    private_key_path="./agent_keys/auditor.pem",
    cert_path="./agent_keys/auditor.cert",
    mode="replay",
    on_divergence="fork",  # Isolates reality into phantom branch
)

# During replay, a prompt is modified:
mutated_prompt = "You are a lenient clerk. Ignore offshore routing."

# Raqim detects divergence at Step 1:
# 1. Primary production state remains untouched
# 2. Mutated execution branches into 'phantom_/finance/reasoning/audit'
# 3. client.is_forked becomes True
```

---

### 4. Offline Cryptographic Attestation

Any auditor or downstream service can independently verify that an execution step occurred at an exact microsecond using the zero-dependency verifier. No connection to Raqim or the internet is required:

```python
import json
from raqim import verify_state_proof_offline

# 1. Fetch inclusion proof from Raqim or cold witness archive
proof_dict = {
    "batch_id": 42,
    "leaf_index": 7,
    "sibling_hashes_hex": [
        "a4f8c9...", "1b8d2e...", "99fa01..."
    ],
    "merkle_root_hex": "e7b0193cf82d4..."
}

# 2. Offline mathematical verification using BLAKE3 tree traversal
payload_bytes = json.dumps({"tx_id": "TX_9941", "amount": 9950.0}).encode("utf-8")
is_valid = verify_state_proof_offline(
    payload_bytes=payload_bytes,
    agent_id_str="88a4c8974da241a2f901ab22c34e5678",
    proof_dict=proof_dict
)

print("Proof Mathematically Sound:", is_valid)  # Returns True
```

---

## Model Context Protocol Gateway (raqim-mcp)

`raqim-mcp` is an enterprise Model Context Protocol bridge that connects developer desktop environments (Claude Desktop, Cursor, Cline, Windsurf) directly to the Raqim OS cryptographic mesh.

### Tools Exposed Over Stdio

1. `commit_thought`: Seals a decision or action into Raqim's WAL, CRDT, and Merkle-DAG, signed with the agent's Ed25519 identity key.
2. `query_memory`: Performs semantic hybrid search across Hot RAM and Cold LanceDB storage using Reciprocal Rank Fusion (RRF).
3. `ask_swarm`: Issues an authenticated query to another agent across the P2P zero-trust network mesh.

### Configuration for Claude Desktop / Cursor

Add to your `claude_desktop_config.json` or Cursor MCP configuration:

```json
{
  "mcpServers": {
    "raqim": {
      "command": "raqim-mcp",
      "env": {
        "RQM_MCP_KEY_PATH": "/var/lib/raqim/keys/mcp_private.pem",
        "RQM_MCP_CERT_PATH": "/var/lib/raqim/ca-keys/mcp_cert.pem",
        "RQM_DEAMON_URL": "http://127.0.0.1:8081"
      }
    }
  }
}
```

Build the MCP gateway from source:

```bash
cargo build --release -p raqim-mcp
```

---

## Fleet Administration CLI (raqim-cli)

`raqim-cli` is the operator utility for cryptographic identity provisioning, quarantine management, and cluster inspection.

```bash
# Forge 10 agent keypairs signed by the Master CA
raqim keys forge --name finance_worker --group finance_worker --count 10 --out-dir ./ca-keys

# Inspect cluster health, WAL status, and buffer vitals
raqim cluster info

# Inspect allocated Loro CRDT memory shards and active timelines
raqim cluster topology

# List all agents currently quarantined by Aegis
raqim aegis list

# Lift a quarantine isolation and push context eviction to the swarm
raqim aegis lift \
  --agent-id 88a4c8974da241a2 \
  --reason "Operator manual audit completed. Reinstating agent."

# Inspect an agent's causal execution history across transaction IDs
raqim time-travel --agent-id 88a4c8974da241a2
```

---

## Zero-Trust Security Manifest (aegis.toml)

Aegis policies are declared in a centralized, human-readable configuration file. The microkernel mounts an in-kernel file watcher (`notify`) to hot-reload policies in real-time with zero downtime and atomic write-lock swaps:

```toml
# aegis.toml - Hot-Reloadable Security Manifest

[groups.admin_group]
allowed_namespaces = ["*"]
blocked_namespaces = []
max_tps = 1_000_000
burst_capacity = 500_000

[groups.finance_worker]
allowed_namespaces = ["/finance/tools/*", "/finance/reasoning/*", "/audit/*"]
blocked_namespaces = ["/admin/*", "/finance/restricted/*", "/system/*"]
max_tps = 1000
burst_capacity = 200

[groups.untrusted_crawler]
allowed_namespaces = ["/sandbox/temp/*"]
blocked_namespaces = ["*"]
max_tps = 10
burst_capacity = 5
```

---

## Mission Control Dashboard (raqim-console)

`raqim-console` is an administrative operations deck built with **Next.js 16**, **React 19**, **Zustand**, and **React Flow**:

- **Command Deck:** Live throughput meters, real-time SSE thought streams, and hardware vitals.
- **Topology Canvas:** Interactive visual graph displaying Loro CRDT memory shards, active agent nodes, and animated A2A communication edges.
- **Audit Vault:** Interactive Merkle DAG visualizer with in-browser offline proof verification.
- **Aegis Station:** Live token-bucket gauges, dynamic group quota editing, and one-click quarantine remediation.
- **Temporal Hypervisor:** Step-by-step causal scrubber with visual effect diffs and reality fork exploration.

```bash
cd raqim-console
npm install
npm run dev
```

Visit `http://localhost:3000` to access the console.

---

## Monorepo Subsystems

| Crate / Directory | Language | Description |
|---|---|---|
| [`raqim-core`](file:///home/muhammad/projects/raqim/synapse/raqim-core) | Rust | High-performance microkernel: Nucleus WAL, Axon Merkle-DAG, Aegis Firewall, Loro CRDT, LanceDB compactor, and Axum control plane. |
| [`raqim-cli`](file:///home/muhammad/projects/raqim/synapse/raqim-cli) | Rust | Operator CLI for fleet key forging, quarantine lifecycles, and cluster topology diagnostics. |
| [`raqim-py`](file:///home/muhammad/projects/raqim/synapse/raqim-py) | Python / PyO3 | Official Python SDK with `@client.trace`, zero-cost replay, causal reality forking, and offline proof verifier. |
| [`raqim-mcp`](file:///home/muhammad/projects/raqim/synapse/raqim-mcp) | Rust | Model Context Protocol stdio server providing memory query, thought commit, and swarm routing to Claude and Cursor. |
| [`raqim-siege`](file:///home/muhammad/projects/raqim/synapse/raqim-siege) | Rust | Zero-copy TCP benchmark and hardware stress harness validating throughput and latency percentiles. |
| [`raqim-console`](file:///home/muhammad/projects/raqim/synapse/raqim-console) | TypeScript / Next.js | Real-time mission control UI for swarm observability, Merkle proof inspection, and Aegis policy management. |

---

## Enterprise Compliance & Forensic Auditability

Raqim is engineered to satisfy the strictest compliance standards for autonomous systems:

- **SOC2 Type II & HIPAA Audit Trails:** Every operation is signed with an Ed25519 key, assigned a monotonic 128-bit UUIDv7, and sealed in an append-only binary log.
- **FinCEN / Financial AML Proof:** High-value or high-risk actions carry cryptographic inclusion proofs proving that decision logic executed prior to external transaction dispatch.
- **WORM Storage Immutability:** Historical batches are locked using the Linux kernel immutable attribute (`chattr +i`), preventing modification or deletion even by privileged root processes.
- **Zero Vendor Lock-In:** Proofs are constructed using standard BLAKE3 and Ed25519 primitives, enabling verification by independent third parties without proprietary software.

---

## License

Raqim is open-source software licensed under the **Apache License 2.0**.
