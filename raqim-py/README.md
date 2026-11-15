# Raqim Python SDK (رَقِيم) — `v0.2.0`

> **Zero-secret egress airlock, pre-execution policy gate, and deterministic flight recorder for autonomous AI agents.**

[![PyPI](https://img.shields.io/pypi/v/raqim.svg)](https://pypi.org/project/raqim/)
[![Python 3.10+](https://img.shields.io/badge/python-3.10+-blue.svg)](https://www.python.org/downloads/)
[![License: Apache-2.0](https://img.shields.io/badge/License-Apache_2.0-blue.svg)](https://opensource.org/licenses/Apache-2.0)

---

## Overview

The `raqim` Python package provides native PyO3/Rust bindings and an async client for the **Raqim Core Daemon (`v0.2.0`)**:

1. **Zero-Secret Egress Airlock (`client.egress_request`):** Route outbound payment and LLM calls (`/v1/egress/{connector}/{path}`) using a short-lived `rqm_v1` Ed25519 workload token while the Rust daemon injects upstream API keys (`sk_live_...`, `OPENAI_API_KEY`) and deterministic `Idempotency-Key` headers.
2. **Pre-Execution Policy Gate:** Enforces route allowlists, strict anti-smuggling JSON parsing, RFC 6901 JSON-Pointer value caps (`/amount <= 50000`), and rolling 1-hour velocity budgets before any side effect leaves the host.
3. **Deterministic `$0.00` Replay & Causal Forking (`@client.trace`):** Replay recorded tool calls and streamed SSE LLM responses in milliseconds with zero upstream API cost, or automatically branch onto a `phantom_` timeline when inputs diverge.
4. **Offline 9-Field BLAKE3 Merkle Verification (`verify_state_proof_offline`):** Verify inclusion proofs offline—including records that have undergone in-place **NDPA 2023 / GDPR** PII crypto-shredding (`payload_bytes=None`).

---

## Installation

```bash
pip install raqim
```

To build from source inside a virtual environment:

```bash
cd raqim-py
maturin develop --release
```

---

## Quickstart

### 1. Zero-Secret Payment & LLM Egress Proxy

```python
import asyncio
from raqim import RaqimClient

async def main():
    client = RaqimClient(
        alias="treasury_primary",
        tenant="fintech_prod",
        private_key_path="./ca-keys/treasury_primary.pem",
        cert_path="./ca-keys/treasury_primary.cert",
        workload_token="rqm_v1.<claims_b64>.<ed25519_sig_b64>",
        daemon_host="127.0.0.1",
        tcp_port=8080,
        http_port=8081,
    )
    await client.boot()

    # Agent holds ZERO Paystack keys; Raqim checks policy, scrubs `/bvn` pre-WAL,
    # binds an Intent UUIDv7 as `Idempotency-Key`, and injects `Bearer sk_live_...`
    resp = await client.egress_request(
        "POST",
        "paystack",
        "/transfer",
        json_body={
            "amount": 25000,
            "currency": "NGN",
            "recipient": "RCP_VENDOR_LAGOS_01",
            "bvn": "22345678901",
        },
        step_ordinal=1,
    )
    print("Status:", resp.status_code, "Sealed TxID:", resp.headers.get("x-raqim-tx-id"))

asyncio.run(main())
```

### 2. Offline Merkle Proof Verification (Pre- & Post-NDPA Redaction)

```python
from raqim import verify_state_proof_offline

# 1. Verify full payload + 9-field execution envelope against signed Merkle root
assert verify_state_proof_offline(
    payload_bytes=raw_response_bytes,
    agent_id_str=client.agent_hex,
    proof_dict=proof,
)

# 2. After NDPA/GDPR in-place erasure (`raqim-cli redact <tx_id>`),
# pass `payload_bytes=None` to verify envelope inclusion without raw PII:
assert verify_state_proof_offline(
    payload_bytes=None,
    agent_id_str=client.agent_hex,
    proof_dict=redacted_proof,
)
```

---

## Repository & Documentation

- **GitHub Repository:** [github.com/raqim-ai/raqim](https://github.com/raqim-ai/raqim)
- **Threat Model:** [docs/THREAT_MODEL.md](https://github.com/raqim-ai/raqim/blob/main/docs/THREAT_MODEL.md)
- **License:** Apache-2.0
