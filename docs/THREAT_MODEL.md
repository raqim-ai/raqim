# Raqim Security & Threat Model (v0.2.0)

## 1. System Scope & Core Security Invariants
Raqim (`raqim-core`) is a single-node execution-integrity substrate, pre-execution policy gate, and cryptographic flight recorder for autonomous AI agent workloads.

It is engineered to uphold four core security invariants:
1. **Identity-Bound Execution:** No operation is evaluated or recorded without cryptographic verification of the caller's identity (Ed25519 signature + CA-signed capability passport or workload token).
2. **Pre-Execution Policy Interdiction:** Policy evaluation (namespace ACLs, rate limits, and payload predicates) occurs *prior* to side-effect dispatch. Denied intents never execute downstream.
3. **Tamper-Evident Causal Auditability:** Every state transition is committed to a CRC32-checksummed Write-Ahead Log (WAL) and sealed into a domain-separated BLAKE3 Merkle tree (`raqim.merkle.v1.leaf`), enabling offline inclusion verification.
4. **Privacy-Preserving Erasure (NDPA / GDPR Compatibility):** Merkle leaves bind to `payload_hash = BLAKE3("raqim.payload.v1", payload_bytes)` rather than raw payload bytes, allowing post-hoc PII redaction (`payload_bytes = []`) without invalidating historical Merkle proofs.

---

## 2. Trust Boundaries

┌────────────────────────────────────────────────────────────────────────────┐
│ BOUNDARY A: UNTRUSTED AGENT ZONE (Container / Process Sandbox)             │
│ • Assumed compromised via Indirect Prompt Injection or malicious inputs.   │
│ • Holds workload identity credentials ONLY (zero upstream API secrets).    │
│ • Isolated inside an internal-only network (internal: true / netns).     │
└─────────────────────────────────────┬──────────────────────────────────────┘
│ Loopback / Internal TCP (:8080, :8081)
▼
┌────────────────────────────────────────────────────────────────────────────┐
│ BOUNDARY B: RAQIM EXECUTION GATEWAY & KERNEL DAEMON (raqim-core)         │
│ • Trusted computing base (TCB) running as a dedicated host/sidecar user.   │
│ • Enforces PolicyEngine rules, injects upstream API secrets, writes WAL.   │
└─────────────────┬───────────────────────────────────────────┬──────────────┘
│ Unix Socket (./vault/admin.sock [0600]) │ Outbound TLS
▼                                           ▼
┌─────────────────────────────────────────┐ ┌────────────────────────────────┐
│ BOUNDARY C: OPERATOR ADMIN PLANE        │ │ BOUNDARY D: EXTERNAL UPSTREAMS │
│ • Host OS Owner + admin.secret Bearer │ │ • OpenAI, Gemini, Stripe,      │
│ • Mints passports, lifts quarantines.   │ │   Paystack, WORM Witness Vault │
└─────────────────────────────────────────┘ └────────────────────────────────┘

---

## 3. Attacker Models & STRIDE Analysis

### Threat 1: Compromised or Prompt-Injected Agent Workload
* **Attacker Capability:** Full control over the agent's prompt context or application-level tool arguments inside Boundary A.
* **Attack Vectors & Mitigations:**
  * *Unauthorized Tool/Endpoint Invocation (Elevation of Privilege):* Mitigated by `PolicyEngine` wildcard namespace ACLs (`blocked_namespaces` takes strict precedence over `allowed_namespaces`, followed by default-deny).
  * *Denial of Wallet / Token Exhaustion (DoS):* Mitigated by lock-free hardware CAS token-bucket rate limiters (`AtomicTokenBucket`) and payload value predicates.
  * *Admin Plane Escalation (Tampering):* Mitigated by physical router separation. Administrative routes (`/v1/admin/*`) are mounted on the Unix Domain Socket (`./vault/admin.sock`, mode `0600`) and require `admin.secret` Bearer authentication; they do not exist on the production Agent Data Plane router.
  * *Replay of Captured Wire Frames (Spoofing/Replay):* Mitigated by a strict $\pm 30\text{s}$ timestamp freshness window and Ed25519 signature verification over canonical request envelopes.

### Threat 2: Malicious or Curious Post-Facto Auditor / DBA
* **Attacker Capability:** Read/write access to exported database records or unanchored log files after execution.
* **Attack Vectors & Mitigations:**
  * *Silent Record Modification or Deletion (Repudiation):* Any 1-byte mutation to `payload_bytes` alters `payload_hash`, invalidating `leaf_hash` and failing `MerkleEngine::verify_inclusion` against the signed batch root.
  * *Physical Disk Bit-Rot or Torn Writes:* Mitigated by per-frame Castagnoli CRC32 checksums in both `production.wal` and `cold_history.seg`, plus atomic tempfile-rename checkpoints (`state_checkpoint.bin`).

### Threat 3: Host Root / Privileged Infrastructure Administrator
* **Attacker Capability:** `root` (`UID 0`) access on the host machine running `raqim-core`.
* **Residual Risk (Honest Boundary):**
  * Local filesystem permissions (`0o600` on keys, `0o400` on local witness files) **cannot** stop a rogue host `root` user from reading memory, extracting local signing keys, or rewriting local files (`production.wal` and `./vault/witnesses/*`) if witness roots are only stored locally.
* **Mitigation Required for Host-Admin Non-Repudiation:**
  * To defend against a malicious host `root` administrator, operators **must** configure `worm_remote_url` to replicate `CertifiedBundleBlock` receipts to an external, compliance-locked WORM object store (e.g., external MinIO or Cloudflare R2 with Object Lock in Compliance Mode) or anchor batch roots to an external RFC 3161 Trusted Timestamping Authority (TSA).

---

## 4. Explicit Non-Goals & Out-of-Scope Threats (v0.2.0)
1. **Cooperative SDK Mode Without Network Isolation:** When using `@client.trace` on a host where the Python process has direct internet access (`0.0.0.0/0`) and holds raw API keys in its environment, a malicious native dependency can open raw sockets and bypass the SDK decorator. Structural enforcement requires running the agent inside an isolated network namespace (`internal: true`) routing solely through the Raqim Egress Gateway.
2. **Semantic NLP Prompt-Injection Classification:** Raqim enforces deterministic structural, identity, rate, and JSON-predicate policies; it does not run probabilistic LLM-judge classifiers over natural language prose.
3. **Side-Channel & Physical Hardware Attacks:** Kernel-level memory scraping (`ptrace` by `root`), Spectre/Meltdown microarchitectural side channels, and physical disk theft without full-disk encryption (LUKS) are outside the scope of the userspace daemon.

