# Changelog

All notable changes to Raqim will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.0.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

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
  - **Phoenix 3-Stage Boot Recovery**: Automatically hydrates state snapshots in `< 5ms`, replays trailing control mutations, and streams trailing WAL thoughts, eliminating reboot state loss even if compaction occurred seconds before power loss.
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
- **Hardware-Enforced Group Commits**: 2ms NVMe `fdatasync` batching yielding 23,355 closed-loop server ACKs/sec.
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

