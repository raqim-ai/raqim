use async_trait::async_trait;

use crate::{api::TimelineNode, OpLog};
// RAQIM COLD STORAGE ABSTRACTION & BINARY SEGMENT ARCHIVE
// Decouples 2PC wal compactor & time-travel Resurrection Engine from heavy
// vector dbs (lancedb) and ML embedding models (ONNX). By default, compacted
// WAL segments are sealed into CRC32-vefified, 16-byte aligned rkyv binary seg files (`SegmentColdStore`) in <1ms.

/// The Unified Cold Storage Contract: Any long-term archive (local bin seg, S3, lancedb) implement
/// this trait so the kernel never blocks on ML interference during compaction
#[async_trait]
pub trait ColdStore: Send + Sync {
    /// Durably archives a slice of the compacted oplog from a rotated WAL seg
    async fn archive_batch(&self, logs: &[OpLog]) -> Result<usize, anyhow::Error>;

    /// Reconstruct the chronological timeline metadata for a specific agent
    async fn fetch_historical_timeline(
        &self,
        agent_hex: &str,
    ) -> Result<Vec<TimelineNode>, anyhow::Error>;

    /// Fetches full historical Oplog for an agent within a trasaction ID window [min_tx, max_tx].
    async fn fetch_historical_oplogs(
        &self,
        agent_hex: &str,
        min_tx_id: u128,
        min_tx_id: u128,
    ) -> Result<Vec<OpLog>, anyhow::Error>;

    /// Point-Lookup of a historical transaction by its UUIDv7 TxID.
    async fn fetch_by_txid(&self, target_tx_id: u128) -> Result<Option<String>, anyhow::Error>;

    /// Appends a system/security audit event to the cold forensic ledger
    async fn log_system_event(&self, event: &SystemEvent);

    async fn get_total_record_count(&self) -> usize;
    async fn get_storage_size_mb(&self) -> f64;
    async fn get_densent_namespace(&self) -> String;

    /// Identifier of the active storage backend (e.g., "SegmentColdStore" or "LanceEngine")
    fn engine_name(&self) -> &str;

    /// Dimension of vectors if semantic indexing is active (0 for pure binary segement store )
    fn vector_dims(&self) -> i32 {
        0
    }
}
