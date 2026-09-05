<div align="center">

# RAQIM OS

**The Sovereign Agent Operating System & Cryptographic Flight Recorder for Enterprise AI Swarms**

Built in Rust · 105,000+ TPS · 2 µs Median Latency · Zero-Cost Deterministic Replay

Apache 2.0 · Python 3.10+ · Docker 35MB · MCP Compatible

---

</div>

## Overview

Modern multi-agent architectures (LangChain, CrewAI, AutoGen, OpenAI Swarm) orchestrate non-deterministic reasoning over ephemeral memory and volatile relational databases. When an autonomous agent hallucinates, causes data corruption, triggers an unauthorized API call, or fails silently in production, engineering teams are left debugging stochastic black boxes — incurring duplicate API token bills and guessing past states.

**Raqim** is an ultra-high-performance microkernel built from the ground up in Rust that serves as an **Agent Operating System (AOS)** and **Cryptographic Flight Recorder**. Every thought, tool invocation, CRDT delta, and external network side-effect is cryptographically bound into an append-only Write-Ahead Log (WAL) and sealed within an in-memory BLAKE3 Merkle-DAG.

With Raqim, AI swarms achieve **sub-microsecond execution recording**, **zero-cost deterministic historical replay**, **causal reality forking**, and **mathematical inclusion proofs** verifiable offline without network dependencies.

---

## Why Raqim?

| Operational Challenge | Traditional Frameworks | Raqim AOS |
|---|---|---|
| **State Persistence** | Volatile RAM, Redis, or SQL rows | Zero-copy binary WAL with hardware io_uring sync |
| **Hallucination Debugging** | Re-run LLMs at full token cost | Deterministic replay from disk in under 1ms at zero cost |
| **Multi-Agent Consensus** | Race conditions and lock contention | Loro CRDT conflict-free causal timeline sharding |
| **Forensic Auditability** | Unverified logs easily altered | Signed Merkle-DAG with WORM storage and chattr +i |
| **Ingress Throughput** | 500 - 2,000 HTTP req/sec | 105,000+ TPS raw TCP over realigned binary frames |
| **Tool Execution Safety** | Trusting agent prompts blindly | Aegis Firewall with Ed25519 token lineage |

---

## Core Capabilities

### Zero-Copy Ingress at 105,000+ TPS

Engineered with `rkyv` zero-copy binary serialization, memory-word realignment (AlignedVec), and hardware NVMe io_uring flushing. Ingest hundreds of thousands of autonomous agent operations per second with 2 microsecond median latency.

### Zero-Cost Deterministic Replay

Every external HTTP call, tool response, and entropy seed is recorded as a cryptographically indexed EffectRecord. During debugging or regression testing, execute historical code paths with **zero LLM token costs** and sub-millisecond turnaround.

### Causal Reality Forking

When prompts, weights, or business logic diverge during replay, Raqim seamlessly isolates the execution branch into a `phantom_` CRDT namespace. Compare alternate universe states side-by-side without mutating primary production data.

### Aegis Cryptographic Firewall

Kernel-level security layer featuring Ed25519 session lineage binding, atomic compare-and-swap token-bucket rate limiters, anti-replay timestamp windows, and distributed quarantine gossip across the Zenoh P2P mesh.

### Mathematical Inclusion Proofs

Every 1,024 agent thoughts are crystallized into a BLAKE3 Merkle root chained to historical parent roots. Generate lightweight mathematical inclusion proofs verifying that an action occurred at an exact microsecond — satisfying strict compliance audits (SOC2, HIPAA, FinCEN).

### Dual-Tier Hybrid Memory

Queries scatter concurrently to an in-memory 10,000-slot vector ring buffer and columnar LanceDB storage. Results are fused using Reciprocal Rank Fusion (RRF) with continuous exponential time decay, boosting recent memories by 25%.

---

## System Architecture

```
                           INCOMING AGENT INGRESS
            (TCP Stream :8080  |  Length-Prefixed rkyv Envelopes)
                                      |
                                      v
                     +------------------------------------+
                     |      AEGIS SECURITY FIREWALL       |
                     |  . Ed25519 Session Handshake       |
                     |  . Anti-Replay Timestamp Audit     |
                     |  . Atomic Token-Bucket (CAS)       |
                     |  . Namespace Access Control        |
                     +----------------+-------------------+
                                      | (Authorized)
                                      v
                     +------------------------------------+
                     |     execute_raqim_cascade()        |
                     |  . Monotonic UUIDv7 Generation     |
                     |  . Zero-Copy Frame Realignment     |
                     +----------+--------+--------+------+
                                |        |        |
           +--------------------+        |        +--------------------+
           v                             v                             v
 +-------------------+       +-------------------+       +-------------------+
 |    NUCLEUS WAL    |       |  AXON MERKLE-DAG  |       |  LORO CRDT BRAIN  |
 |  Sequential NVMe  |       |  1,024-Leaf Tree  |       |   Conflict-Free   |
 |  io_uring Flusher |       |  BLAKE3 KDF Root  |       |  Swarm Sharding   |
 |  CRC32 Integrity  |       |  Inclusion Proofs |       |  Diff Dispatch    |
 +---------+---------+       +---------+---------+       +---------+---------+
           |                           |                           |
           v                           v                           v
 +-------------------+       +-------------------+       +-------------------+
 |   2PC COMPACTOR   |       |   WORM WITNESS    |       |    ZENOH MESH     |
 |  Hot WAL -> Lance |       |  Ed25519 Signed   |       |  P2P Gossip Mesh  |
 |  Columnar Vector  |       |  Linux chattr +i  |       |  A2A RPC Routing  |
 +-------------------+       +-------------------+       +-------------------+
```

---

## Quickstart

### 1. Launch with Docker Compose

```yaml
# docker-compose.yml
version: "3.8"

services:
  raqim-core:
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
```

Start the cluster daemon:

```bash
docker compose up -d
```

Verify the microkernel is active:

```bash
curl -s http://localhost:8081/v1/dashboard/cards | jq .
```

---

### 2. Fleet Administration via raqim-cli

```bash
# Forge 10 agent keypairs with signed capability certificates
raqim-cli forge --count 10 --group finance_worker --out ./keys

# Inspect cluster topology and allocated Loro CRDT shards
raqim-cli cluster topology

# Inspect quarantined agents
raqim-cli aegis list

# Lift a quarantine with context eviction
raqim-cli aegis lift \
  --agent-id 88a4c8974da241a2 \
  --reason "Operator manual reset after review"

# Inspect an agent's causal execution timeline
raqim-cli time-travel --agent-id 88a4c8974da241a2
```

---

## Python SDK and Runtime

Install the official Python client:

```bash
pip install raqim httpx
```

### Production Tracing (Record Mode)

Decorate your LLM pipelines with the `@client.trace` interface:

```python
import asyncio
import os
from raqim import RaqimClient
from google import genai

# Initialize sovereign agent identity
client = RaqimClient(
    alias="fraud_analyst",
    tenant="production",
    private_key_path="./keys/fraud_analyst.pem",
    cert_path="./keys/fraud_analyst.cert",
    mode="record",  # Switch to "replay" for zero-cost execution
)

ai = genai.Client(api_key=os.getenv("GEMINI_API_KEY"))

# Trace execution steps into Raqim's Merkle-DAG
@client.trace(namespace="/finance/sanctions")
async def screen_transaction(account_id: str, amount: float) -> dict:
    prompt = f"Evaluate AML risk for account {account_id} transferring ${amount:,.2f}."
    response = await asyncio.to_thread(
        ai.models.generate_content,
        model="gemini-2.5-flash",
        contents=prompt
    )
    return {
        "account": account_id,
        "amount": amount,
        "flagged": amount >= 10000.0,
        "assessment": response.text,
    }

async def main():
    await client.boot()
    result = await screen_transaction("ACC-OFFSHORE-9912", 14500.00)
    print("Result:", result)

if __name__ == "__main__":
    asyncio.run(main())
```

### Zero-Cost Deterministic Replay

Switch `mode="replay"` on the client. Raqim intercepts external API calls, validates the BLAKE3 call signature, and serves cached results directly from disk in under 0.3ms — zero network calls, zero token fees:

```python
client = RaqimClient(
    alias="fraud_analyst",
    tenant="production",
    private_key_path="./keys/fraud_analyst.pem",
    mode="replay",  # Identical deterministic output at zero cost
)
```

---

## Benchmarks

Benchmarked using `raqim-siege` under continuous saturated load on an Intel Core i7-13700K with PCIe Gen4 NVMe storage:

| Metric | Result | Significance |
|---|---|---|
| Peak Ingress Velocity | **105,240 TPS** | Exceeds global financial clearinghouse rates |
| Median Latency (p50) | **2 microseconds** | Sub-microsecond memory-mapped pointer handoff |
| Tail Latency (p99) | **18 microseconds** | Zero-allocation alignment prevents GC stalls |
| Worst-Case (p99.9) | **45 microseconds** | Non-blocking ring buffers under 50 TCP sockets |
| Deterministic Replay | **under 0.3 ms** | Served from local memory-mapped cache (zero cost) |
| Phoenix Crash Recovery | **500k logs in under 1.5s** | CRC32-validated binary WAL hydration |

Run the benchmark locally:

```bash
cargo run --release -p raqim-siege -- \
  --concurrency 50 \
  --rounds 2000 \
  --agents 10 \
  --addr 127.0.0.1:8080
```

---

## Mission Control Console

`raqim-console` is an enterprise administrative interface built with Next.js 16, React 19, Zustand, and React Flow:

```bash
cd raqim-console
npm install
npm run dev
```

Visit `http://localhost:3000` to access:

- **Command Deck** — Real-time throughput gauges, live SSE thought stream, and 1Hz hardware vitals.
- **Topology Canvas** — Interactive graph displaying Loro CRDT memory shards, active agent nodes, and animated A2A beam edges.
- **Audit Vault** — Deep Merkle inclusion tree visualizer with offline in-browser proof verification.
- **Aegis Station** — Live token-bucket gauges, dynamic group quota editor, and quarantine remediation console.
- **Temporal Hypervisor** — Step-by-step causal scrubber with visual effect diffs and universe branching triggers.

---

## Security and Compliance

Raqim is engineered to satisfy the most stringent compliance standards for autonomous systems:

- **SOC2 Type II and HIPAA Audit Trails** — Every operation is bound to a 128-bit UUIDv7, cryptographically signed, and archived with tamper evidence.
- **WORM Immutability** — Batches anchored by the Witness Engine are locked using the Linux kernel immutable attribute (chattr +i), preventing modification even by root.
- **Cryptographic Independence** — Merkle inclusion proofs can be verified completely offline using standalone BLAKE3 algorithms without contacting the Raqim cluster.
- **Model Context Protocol (MCP)** — Built-in universal translator (raqim-mcp) allowing Claude Desktop and Cursor to operate inside the secure Raqim governance mesh.

---

## Ecosystem

| Component | Description |
|---|---|
| `raqim-core` | Rust microkernel — WAL, Merkle-DAG, Aegis, CRDT, and Axum Control Plane |
| `raqim-cli` | Operator CLI for key minting, fleet provisioning, and quarantine control |
| `raqim-py` | PyO3 native C-extension, async client, and deterministic replay engine |
| `raqim-mcp` | Model Context Protocol stdio server for LLM interfaces (Claude, Cursor) |
| `raqim-siege` | High-concurrency zero-copy TCP stress harness and latency benchmarker |
| `raqim-console` | Next.js 16 / React 19 real-time mission control dashboard |

For the full technical specification and low-level subsystem reference, see `ARCHITECTURE_DEEP_DIVE.md` in this repository.

---

## License

Raqim is open-source software distributed under the **Apache License 2.0**.
