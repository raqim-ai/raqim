use std::{format, matches};
use std::{path::PathBuf, sync::atomic::AtomicUsize};

use async_trait::async_trait;
use tokio::sync::Mutex;

use crate::AgentStatus;
use crate::{api::TimelineNode, OpLog, SystemEvent};

use memmap2::MmapOptions;
use rkyv::{to_bytes, Archive};
use std::collections::HashMap;
use std::fs::{self, File, OpenOptions};
use std::io::Write;
use std::path::Path;
use std::sync::atomic::Ordering;
use std::time::{SystemTime, UNIX_EPOCH};

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
    async fn get_densest_namespace(&self) -> String;

    /// Identifier of the active storage backend (e.g., "SegmentColdStore" or "LanceEngine")
    fn engine_name(&self) -> &str;

    /// Dimension of vectors if semantic indexing is active (0 for pure binary segement store )
    fn vector_dims(&self) -> i32 {
        0
    }
}

// ============================================================
// DEFAULT ENGINE: ZERO-DEPENDENCY CRC32 BINARY SEGMENT STORE
// =============================================================

pub struct SegmentColdStore {
    storage_dir: PathBuf,
    history_file_path: PathBuf,
    audit_file_path: PathBuf,
    write_lock: Mutex<()>,
    cached_record_count: AtomicUsize,
}

impl SegmentColdStore {
    pub fn new(base_dir: &str) -> Self {
        let dir = PathBuf::from(base_dir);
        let _ = fs::create_dir_all(&dir);

        let history_file_path = dir.join("cold_history.seg");
        let audit_file_path = dir.join("system_audit.jsonl");

        let initial_count = Self::count_records_on_disk(&history_file_path);

        Self {
            storage_dir: dir,
            history_file_path,
            audit_file_path,
            write_lock: Mutex::new(()),
            cached_record_count: AtomicUsize::new(initial_count),
        }
    }

    fn count_records_on_disk(path: &Path) -> usize {
        let mut count = 0;
        let _ = Self::scan_segment_file(path, |batch| {
            count += batch.len();
        });

        count
    }

    /// Zero-copy mmap scanner over the cold segment file.
    /// Physics: Uses the exact [4B len] [4B crc32][rkyv Vec<OpLog>] framing as Nucleus WAL.
    fn scan_segment_file<F>(path: &Path, mut callback: F) -> Result<(), anyhow::Error>
    where
        F: FnMut(&rkyv::Archived<Vec<OpLog>>),
    {
        if !path.exists() {
            return Ok(());
        }

        let file = File::open(path)?;
        if file.metadata()?.len() < 8 {
            return Ok(());
        }

        let mmap = unsafe { MmapOptions::new().map(&file)? };
        let mut offset = 0;
        let mut aligned_buf = rkyv::util::AlignedVec::<16>::new();

        while offset + 8 < mmap.len() {
            let entry_len =
                u32::from_le_bytes(mmap[offset..offset + 4].try_into().unwrap()) as usize;
            let expected_crc = u32::from_le_bytes(mmap[offset + 4..offset + 8].try_into().unwrap());
            let frame_total = 8 + entry_len;

            if offset + frame_total > mmap.len() {
                break;
            }

            let entry_slice = &mmap[offset + 8..offset + frame_total];
            if crc32fast::hash(entry_slice) == expected_crc {
                aligned_buf.clear();
                aligned_buf.extend_from_slice(entry_slice);

                if let Ok(archived_batch) = rkyv::access::<
                    <Vec<OpLog> as Archive>::Archived,
                    rkyv::rancor::Error,
                >(&aligned_buf)
                {
                    callback(archived_batch);
                }
            }
            offset += frame_total;
        }
        Ok(())
    }
}

#[async_trait]
impl ColdStore for SegmentColdStore {
    async fn archive_batch(&self, logs: &[OpLog]) -> Result<usize, anyhow::Error> {
        if logs.is_empty() {
            return Ok(0);
        }

        let _guard = self.write_lock.lock().await;

        let payload_bytes = to_bytes::<rkyv::rancor::Error>(&logs.to_vec())
            .map_err(|e| anyhow::anyhow!("rkyv cold serialization failed: {}", e))?
            .into_vec();

        let len_bytes = (payload_bytes.len() as u32).to_le_bytes();
        let crc_bytes = crc32fast::hash(&payload_bytes).to_le_bytes();

        let mut file = OpenOptions::new()
            .create(true)
            .append(true)
            .open(&self.history_file_path)?;

        file.write_all(&len_bytes)?;
        file.write_all(&crc_bytes)?;
        file.write_all(&payload_bytes)?;
        file.sync_data()?;

        let count = logs.len();
        self.cached_record_count.fetch_add(count, Ordering::Relaxed);

        Ok(count)
    }

    async fn fetch_historical_timeline(
        &self,
        agent_hex: &str,
    ) -> Result<Vec<TimelineNode>, anyhow::Error> {
        let mut nodes = Vec::new();
        let target_bytes = hex::decode(agent_hex).unwrap_or_default();

        Self::scan_segment_file(&self.history_file_path, |archived_batch| {
            for log in archived_batch.as_slice() {
                if log.agent_id.as_slice() == target_bytes.as_slice() {
                    let status_str = match &log.state.status {
                        rkyv::Archived::<AgentStatus>::Idle => "IDLE",
                        rkyv::Archived::<AgentStatus>::Reasoning => "REASONING",
                        rkyv::Archived::<AgentStatus>::Halted => "HALTED",
                        rkyv::Archived::<AgentStatus>::ToolExecution => "TOOL_EXEC",
                    };
                    nodes.push(TimelineNode {
                        tx_id: log.state.transaction_id.to_native(),
                        timestamp: log.state.timestamp.to_native().to_string(),
                        agent_status: status_str.to_string(),
                        payload_preview: log.state.text.as_str().to_string(),
                    });
                }
            }
        })?;

        Ok(nodes)
    }

    async fn fetch_historical_oplogs(
        &self,
        agent_hex: &str,
        min_tx_id: u128,
        max_tx_id: u128,
    ) -> Result<Vec<OpLog>, anyhow::Error> {
        let mut logs = Vec::new();
        let target_bytes = hex::decode(agent_hex).unwrap_or_default();

        Self::scan_segment_file(&self.history_file_path, |archived_batch| {
            for archived_log in archived_batch.as_slice() {
                let tx = archived_log.state.transaction_id.to_native();
                if tx >= min_tx_id
                    && tx <= max_tx_id
                    && archived_log.agent_id.as_slice() == target_bytes.as_slice()
                {
                    if let Ok(owned) = rkyv::deserialize::<OpLog, rkyv::rancor::Error>(archived_log)
                    {
                        logs.push(owned);
                    }
                }
            }
        })?;

        Ok(logs)
    }

    async fn fetch_by_txid(&self, target_tx_id: u128) -> Result<Option<String>, anyhow::Error> {
        let mut found = None;
        Self::scan_segment_file(&self.history_file_path, |archived_batch| {
            if found.is_some() {
                return;
            }

            for log in archived_batch.as_slice() {
                if log.state.transaction_id.to_native() == target_tx_id {
                    found = Some(format!(
                        "[COLD SEGMENT] TxID: {:032x} | Text: {} ",
                        target_tx_id,
                        log.state.text.as_str()
                    ));
                    break;
                }
            }
        })?;

        Ok(found)
    }

    async fn log_system_event(&self, event: &SystemEvent) {
        if matches!(event, SystemEvent::ThoughtCommitted { .. }) {
            return;
        }

        let ts = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_millis();

        if let Ok(event_json) = serde_json::to_string(event) {
            let line = format!("{{\"timestamp\":{},\"event\":{}}}\n", ts, event_json);

            let _guard = self.write_lock.lock().await;
            if let Ok(mut file) = OpenOptions::new()
                .create(true)
                .append(true)
                .open(&self.audit_file_path)
            {
                let _ = file.write_all(line.as_bytes());
            }
        }
    }

    async fn get_total_record_count(&self) -> usize {
        self.cached_record_count.load(Ordering::Relaxed)
    }

    async fn get_storage_size_mb(&self) -> f64 {
        fs::metadata(&self.history_file_path)
            .map(|m| m.len() as f64 / (1024.0 * 1024.0))
            .unwrap_or(0.0)
    }

    async fn get_densest_namespace(&self) -> String {
        let mut counts: HashMap<String, usize> = HashMap::new();
        let mut total = 0usize;

        let _ = Self::scan_segment_file(&self.history_file_path, |batch| {
            for log in batch.as_slice() {
                *counts
                    .entry(log.state.namespace.as_str().to_string())
                    .or_insert(0) += 1;
                total += 1;
            }
        });

        if total == 0 {
            return "Empty (0%)".to_string();
        }

        counts
            .into_iter()
            .max_by_key(|&(_, count)| count)
            .map(|(ns, count)| format!("{} ({:.1}%)", ns, (count as f64 / total as f64) * 100.0))
            .unwrap_or_else(|| "Empty (0%)".to_string())
    }

    fn engine_name(&self) -> &str {
        "SegmentColdStore (Zero-Copy rkyv)"
    }
}
