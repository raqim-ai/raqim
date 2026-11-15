# Changelog

All notable changes to Raqim will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.0.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

---

## [0.2.0] - 2026-11-14

### Added
- **Zero-Secret Egress Gateway ([`src/egress.rs`](raqim-core/src/egress.rs), `ANY /v1/egress/{connector}/{*subpath}`)**:
  - Declarative named upstream reverse-proxy connectors in `raqim.toml` (`upstream_url`, `secret_env`, `auth_header`, `auth_prefix`, `idempotency_header`, `allowed_routes`, `redact_pointers`).
  - Automatic secret injection exclusively from daemon memory; strips agent tokens (`Authorization`, `x-api-key`, `x-goog-api-key`) before upstream dispatch.
  - Deterministic `Idempotency-Key` injection bound to each Write-Ahead `Intent` UUIDv7 transaction ID.
  - Zero-Stall SSE Streaming Byte-Tee (`text/event-stream`) with inline WAL capture and language-agnostic `$0.00` Gateway Replay (`X-Raqim-Mode: replay`, `x-raqim-replay: HIT/MISS`).
- **Standard Ed25519 Workload Tokens ([`src/token.rs`](raqim-core/src/token.rs))**:
  - `rqm_v1.<claims_b64>.<sig_b64>` tokens verified over raw ASCII bytes prior to JSON parsing.
- **The 4 Concentric Policy Locks ([`src/policy.rs`](raqim-core/src/policy.rs))**:
  - `StrictJson` recursive deserializer rejecting duplicate-key JSON smuggling attacks at any depth.
  - RFC 6901 JSON-Pointer value predicates (`min_value`, `max_value`, `allowed_values`) and rolling 1-hour cumulative velocity budgets (`max_hourly_sum`).
  - Per-agent isolated lock-free CAS token buckets with two-tier escalation (`429 RateLimited` on bursts → Quarantine after `50` consecutive flood strikes).
- **O(1) Indexed `ColdStore` Trait & NDPA / GDPR Erasure ([`src/cold_store.rs`](raqim-core/src/cold_store.rs))**:
  - Zero-dependency `SegmentColdStore` (`cold_history.seg` + 44-byte fixed-width `cold_history.idx` + in-memory `DashMap` index).
  - 128-bit CSPRNG `payload_salt` (`raqim.payload.v1`) and in-place on-disk redaction (`POST /v1/admin/redact/{tx_id}`) preserving Merkle inclusion proof validity.
- **Hardened Operator CLI ([`raqim-cli`](raqim-cli/src/main.rs))**:
  - Offline key, certificate, and `rqm_v1` workload token forging (`raqim-cli keys forge`).
  - Authenticated Unix Domain Socket (`./vault/admin.sock`, mode `0600`) operator commands: `doctor [--in-doubt]`, `tail`, `policy list|lift|metrics`, `redact`, `compact`, and `export`.
- **Modular Security & Durability Test Suite ([`raqim-core/tests/`](raqim-core/tests/))**:
  - Added `6` integration test suites (`10` tests passing) verifying WAL durability, mid-file corruption halts, full-envelope Merkle proofs, odd-leaf collision resistance, authenticate-first policy anti-DoS, atomic journal rotation, NDPA redaction, and live egress secret-grep invariants.

### Changed
- **Architecture Consolidation & Boot Performance**:
  - Consolidated `raqim-core` down to `199` unique transitive Rust crates (`cargo tree -p raqim-core`) with `9.80 ms` cold boot/hydration latency and `71.10 ms` full `SIGKILL` Phoenix recovery (`7,175.65 req/sec` true `fdatasync` TCP throughput).
  - Replaced `OpLog`, `AgentState`, `AgentStatus`, and `EffectRecord` with a single unified [`ExecutionRecord`](raqim-core/src/lib.rs) (`Intent`, `Outcome`, `Denied`, `System`).
  - Renamed kernel modules to direct systems primitives: `wal.rs`, `merkle.rs`, `policy.rs`, `effect_store.rs`.
  - Moved `lancedb`, `fastembed`, `arrow-*`, and `hot_memory` behind an off-by-default `vector_embedding` Cargo feature.
  - Rewrote `raqim-py`, `raqim-siege`, `Dockerfile`, `docker-compose.yml`, and the 6-Act `demo.py` showcase for `v0.2.0`.

### Security Fixes
- **Admin Plane Isolation**: Physically separated `build_data_router` (`:8081`) from `build_admin_router` (`./vault/admin.sock`, mode `0600`); removed `/v1/admin/ca/mint` and permissive `CorsLayer` from the daemon; enforced constant-time Bearer `admin.secret` validation on all admin routes.
- **Synchronous `fdatasync` WAL Durability**: Replaced fire-and-forget WAL channel writes with `oneshot`-acknowledged `sync_data()` commits and fail-closed engine poisoning on disk I/O failure.
- **Mid-File WAL Corruption Protection**: Distinguished mid-file CRC-32 corruption (halts boot with `WalError::MidFileCorruption`) from tail torn writes (truncates cleanly) and enforced `MAX_WAL_FRAME_BYTES` (16 MiB).
- **Eliminated Confused Deputy Path Spoofing**: Removed unsigned `intent_path` from `IngressEnvelope`; `PolicyEngine` authorizes `record.namespace` directly from signed `record_bytes`.
- **Authenticate-First Anti-DoS**: Reordered signature verification before quarantine checks so unauthenticated spoofed packets return `401` and never quarantine honest agents.
- **Full-Envelope Merkle Commitment & Odd-Leaf Fix**: Bound all 9 envelope fields into `raqim.merkle.v1.leaf` and added `raqim.merkle.v1.node_odd` domain separation to eliminate CVE-2012-2459 odd-leaf duplication collisions.
- **Atomic Control Journal Compaction**: Added `control_journal.bin.compacting` rotation prior to checkpoint snapshotting so concurrent quarantines are never lost.
- **Unified `RaqimConfig` Schema**: Removed mismatched `RaqimTomlProxy` that caused a panic on second boot after bootstrapping `raqim.toml`.

### Removed
- Removed `loro` CRDT state shards, `zenoh` P2P multicast mesh, and A2A WebSocket routing from `raqim-core` and `raqim-py`.
- Archived legacy `raqim-console` and `raqim-mcp` crates into [`archive/`](archive/).

---

## [0.1.2] - 2026-09-24

### Added
- **Native OpenTelemetry (OTel) Export Engine (`client.export_to_otel()`)**:
  - Full projection of Raqim's execution-integrity DAG into CNCF-compliant OTLP JSON traces (`ExportTraceServiceRequest`).
  - Seamless push to any standard OTLP collector (Jaeger, Grafana Tempo, Datadog, Langfuse) with zero application code refactors.
  - Automatic extraction of step ordinals (`raqim.step_ordinal`) from payload previews.
  - Client-side token enrichment: captures `gen_ai.usage.input_tokens`, `gen_ai.usage.output_tokens`, and `gen_ai.request.model`.
  - Cryptographic TraceID derivation: Blake3 domain-separated hashing over `agent_hex`, `tenant_id`, and initial transaction ID ensures unique, deterministic trace sessions.
- **Zero-Amnesia State Checkpointing & Control Journaling**:
  - **`StateCheckpoint` (`./vault/state_checkpoint.bin`)**: High-performance binary snapshot capturing consolidated in-memory state during WAL compaction (Loro CRDT binary documents, Axon Merkle DAG counters, deterministic effect caches, Aegis quarantine blocklists, and active agent session tables).
  - **`ControlJournal` (`./vault/control_journal.bin`)**: CRC32-checksummed append-only journal capturing discrete control mutations between compaction cycles.
  - **Phoenix 3-Stage Boot Recovery**: Designed for sub-5ms in-memory state reconstruction, replays trailing control mutations, and streams trailing WAL thoughts, eliminating reboot state loss even if compaction occurred seconds before power loss.
- **Automated Durability & OTel Smoke Tests**: Added `tests/smoke_test_v012.rs` verifying snapshot roundtrips, journal truncation, and OTLP trace projections.

### Changed
- **Compactor 2PC State Machine**: `WalCompactor` now snapshots active RAM state before marking manifests as `Committed` and safely removing rotated WAL segments.
- **Storage Tier Separation**: Clarified LanceDB's role strictly as an OLAP analytical lakehouse and vector store; removed LanceDB from the recovery critical path.
- **Aegis Quarantine Persistence**: Replaced ad-hoc disk JSON file I/O with durable `ControlJournal` mutations.

### Fixed
- Fixed integer slicing bug in OTel step ordinal extraction.
- Fixed timestamp scaling across seconds, milliseconds, and nanoseconds magnitudes.
- Fixed unawaited asynchronous HTTP calls and connection pooling leaks in Python SDK (`raqim-py`).
- Harmonized quarantine file path definitions across kernel modules.

---

## [0.1.1] - 2026-09-17

### Added
- **Group-Commit WAL Synchronization**: 2ms NVMe `fdatasync` batching yielding 23,355 closed-loop server ACKs/sec.
- **Automatic Torn-Frame Recovery**: Pre-flight recovery scanner truncates corrupted frames from previous ungraceful OS shutdowns.
- **Apache-2.0 License**: Project officially open-sourced under Apache-2.0.

---

## [0.1.0] - 2026-09-10

### Added
- Initial public release of Raqim Core and Python SDK.
- Ed25519 cryptographic identity and swarm capability passports.
- Aegis pre-execution policy firewall with lock-free atomic token-bucket rate limiting.
- Zero-copy `rkyv` TCP ingress and append-only Nucleus WAL.
- In-memory Loro CRDT shards for multi-agent causal convergence.
- BLAKE3 domain-separated Merkle DAG (`Axon`) with offline inclusion proof attestation.
- $0 deterministic replay engine and causal reality branching (`phantom_` namespaces).

