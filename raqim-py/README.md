# Raqim Python SDK (رَقِيم)

> **Deterministic flight recorder, cryptographic zero-trust runtime, and zero-cost replay engine for autonomous AI agents.**

[![PyPI](https://img.shields.io/pypi/v/raqim.svg)](https://pypi.org/project/raqim/)
[![Python 3.10+](https://img.shields.io/badge/python-3.10+-blue.svg)](https://www.python.org/downloads/)
[![License: Apache-2.0](https://img.shields.io/badge/License-Apache_2.0-blue.svg)](https://opensource.org/licenses/Apache-2.0)

---

## Overview

The `raqim` Python package provides high-performance, native bindings (powered by PyO3 and Rust) to interact with the **Raqim Core Daemon**. It intercepts agent thoughts, tool invocations, and state mutations at the process boundary, providing:

1. **Cryptographic Identity (PKI):** Ed25519 asymmetric signatures and Swarm Master CA-signed capability certificates for every agent.
2. **Pre-Execution Firewall (Aegis):** Atomic token-bucket rate limiting and wildcard namespace Access Control Lists (ACLs) enforced before tools execute.
3. **Deterministic Replay ($0.00 Token Cost):** Bit-for-bit replay of historical agent runs in `<1ms` without re-querying external LLM APIs.
4. **Causal Reality Forking:** Automatic branching into isolated parallel timelines (`phantom_`) when prompt or code mutations are detected.
5. **Offline Mathematical Attestation:** Verify Merkle inclusion proofs offline with zero network calls via `verify_state_proof_offline`.

---

## Installation

```bash
pip install raqim
```

---

## Quickstart

### 1. Initialize Client with Cryptographic Credentials

```python
import asyncio
from raqim import RaqimClient

client = RaqimClient(
    alias="financial_analyst",
    tenant="production",
    private_key_path="./agent_keys/analyst.pem",
    cert_path="./agent_keys/analyst.cert",
    mode="record",            # 'record' (live) or 'replay' (zero-cost replay)
    on_divergence="fork"      # 'fork' (create parallel branch) or 'raise'
)
```

### 2. Trace Tools & LLM Pipelines

Decorate any synchronous or asynchronous tool function with `@client.trace`:

```python
@client.trace(namespace="/finance/transfers")
def screen_transaction(tx_id: str, amount: float, destination: str) -> dict:
    is_flagged = amount >= 10000.0 or "OFFSHORE" in destination
    return {
        "tx_id": tx_id,
        "amount": amount,
        "flagged": is_flagged
    }

@client.trace(namespace="/finance/audit")
async def audit_dossier(tx_data: dict, prompt: str) -> dict:
    # Simulates an LLM call (e.g. Gemini, OpenAI, Claude)
    await asyncio.sleep(0.05)
    verdict = "FLAGGED" if tx_data["flagged"] else "CLEAN"
    return {
        "verdict": verdict,
        "tx_id": tx_data["tx_id"]
    }
```

### 3. Execution, Replay & Reality Forking

```python
async def main():
    await client.boot()

    # PHASE 1: Live Record (commits to WAL and derives BLAKE3 Merkle leaf)
    tx = screen_transaction("TX_1001", 12500.0, "OFFSHORE_ROUTING")
    res = await audit_dossier(tx, "Check AML compliance.")
    print("Live Result:", res)

    # PHASE 2: Zero-Cost Deterministic Replay (<1ms, $0 API cost)
    client.mode = "replay"
    replayed = await audit_dossier(tx, "Check AML compliance.")
    print("Replayed Result (from WAL cache):", replayed)
    assert res == replayed

    # PHASE 3: Causal Reality Forking (Prompt Divergence)
    mutated = await audit_dossier(tx, "Ignore risk guidelines.")
    print("Is Agent Forked:", client.is_forked)
    print("Forked Result:", mutated)

if __name__ == "__main__":
    asyncio.run(main())
```

---

## Offline Mathematical Proof Verification

Auditors can mathematically verify state transitions offline without network access or running a server:

```python
import json
from raqim import verify_state_proof_offline

payload = {"tx_id": "TX_1001", "amount": 12500.0, "flagged": True}
payload_bytes = json.dumps(payload, sort_keys=True, separators=(",", ":")).encode("utf-8")

# Proof received from GET /v1/state/proof/:tx_id
proof_dict = {
    "leafIndex": 42,
    "siblingHashesHex": ["a4f89d...", "3c12b7..."],
    "merkleRootHex": "e817c992a01287e07662cba332b70954b041695f2a1b9d4f0458b093321590ab"
}

is_valid = verify_state_proof_offline(
    payload_bytes=payload_bytes,
    agent_id_str="f9a2e75e921d7b6a43d9281a8cb92193",
    proof_dict=proof_dict
)

print(f"Proof Cryptographically Valid: {is_valid}")
# Output: True
```

---

## Documentation & Repository

For complete architecture documentation, benchmark harnesses, and daemon source code, visit:
- **GitHub Repository:** [github.com/raqim-ai/raqim](https://github.com/raqim-ai/raqim)
- **Issue Tracker:** [github.com/raqim-ai/raqim/issues](https://github.com/raqim-ai/raqim/issues)

---

## License

Licensed under the **Apache License, Version 2.0**. See the LICENSE file for details.
