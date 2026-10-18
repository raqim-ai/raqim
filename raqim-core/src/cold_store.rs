use crate::api::TimelineNode;
use crate::{AgentStatus, OpLog, SystemEvent};
use async_trait::async_trait;
use dashmap::DashMap;
use memmap2::MmapOptions;
use rkyv::{to_bytes, Archive};
use std::fs::{self, File, OpenOptions};
use std::io::{Read, Seek, SeekFrom, Write};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::{SystemTime, UNIX_EPOCH};
use tokio::sync::Mutex;

// RAQIM COLD STORAGE ABSTRACTION & BINARY SEGMENT ARCHIVE
// Decouples 2PC wal compactor & time-travel Resurrection Engine from heavy
// vector dbs (lancedb) and ML embedding models (ONNX). By default, compacted
// WAL segments are sealed into CRC32-vefified, 16-byte aligned rkyv binary seg files (`SegmentColdStore`) in <1ms.

// Coordinate of a single serialized Oplog frame inside `cold_store.seg`
#[derive(Debug, Clone, Copy)]
pub struct FramePointer {
    pub tx_id: u128,
    pub offset: u64,
    pub total_frame_bytes: u32,
}

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
        max_tx_id: u128,
    ) -> Result<Vec<OpLog>, anyhow::Error>;

    /// Point-Lookup of a historical transaction by its UUIDv7 TxID.
    async fn fetch_by_txid(&self, target_tx_id: u128) -> Result<Option<OpLog>, anyhow::Error>;

    /// Appends a system/security audit event to the cold forensic ledger
    async fn log_system_event(&self, event: &SystemEvent);

    async fn get_max_tx_id_for_agent(&self, agent_hex: &str) -> u128;

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
    history_file_path: PathBuf,
    audit_file_path: PathBuf,
    index_file_path: PathBuf,
    /// Serializes concurrent file appends across Tokio worker threads.
    write_lock: Mutex<()>,

    /// 0(1) in-mem map: tx_id -> exact byte offset & length on disk
    tx_index: DashMap<u128, FramePointer>,
    agent_index: DashMap<[u8; 16], Vec<FramePointer>>,

    /// 0(1) namespace freq counter for instant tele without scanning disk
    namespace_counts: DashMap<String, usize>,
    cached_record_count: AtomicUsize,
}

impl SegmentColdStore {
    pub fn new(base_dir: &str) -> Self {
        let dir = PathBuf::from(base_dir);
        let _ = fs::create_dir_all(&dir);

        let history_file_path = dir.join("cold_history.seg");
        let index_file_path = dir.join("cold_history.idx");
        let audit_file_path = dir.join("system_audit.jsonl");

        let store = Self {
            history_file_path,
            index_file_path,
            audit_file_path,
            write_lock: Mutex::new(()),
            tx_index: DashMap::new(),
            agent_index: DashMap::new(),
            namespace_counts: DashMap::new(),
            cached_record_count: AtomicUsize::new(0),
        };

        store.load_or_rebuild_index_on_boot();
        store
    }
    /// Loads the compact 44-byte-per-record `.idx` file into RAM on startup.  
    /// If `.idx` is missing but `.seg` exists, rebuilds the `.idx` automatically.
    fn load_or_rebuild_index_on_boot(&self) {
        if self.index_file_path.exists() {
            if let Ok(idx_bytes) = fs::read(&self.index_file_path) {
                let mut cursor = 0;
                let mut count = 0;
                // Each index record is 44 bytes: [16B tx_id][16B agent_id][8B offset][4B total_len]
                while cursor + 44 <= idx_bytes.len() {
                    let tx_id =
                        u128::from_le_bytes(idx_bytes[cursor..cursor + 16].try_into().unwrap());
                    let agent_id: [u8; 16] =
                        idx_bytes[cursor + 16..cursor + 32].try_into().unwrap();
                    let offset =
                        u64::from_le_bytes(idx_bytes[cursor + 32..cursor + 40].try_into().unwrap());
                    let total_frame_bytes =
                        u32::from_le_bytes(idx_bytes[cursor + 40..cursor + 44].try_into().unwrap());

                    let ptr = FramePointer {
                        tx_id,
                        offset,
                        total_frame_bytes,
                    };
                    self.tx_index.insert(tx_id, ptr);
                    self.agent_index.entry(agent_id).or_default().push(ptr);

                    cursor += 44;
                    count += 1;
                }
                self.cached_record_count.store(count, Ordering::Relaxed);
                return;
            }
        }

        // Fallback: If .seg exists without .idx, scan .seg once to build the index
        if self.history_file_path.exists() {
            if let Ok(seg_bytes) = fs::read(&self.history_file_path) {
                let mut offset = 0usize;
                let mut count = 0usize;
                let mut idx_file = OpenOptions::new()
                    .create(true)
                    .write(true)
                    .truncate(true)
                    .open(&self.index_file_path)
                    .ok();

                let mut aligned_buf = rkyv::util::AlignedVec::<16>::new();

                while offset + 8 <= seg_bytes.len() {
                    let payload_len =
                        u32::from_le_bytes(seg_bytes[offset..offset + 4].try_into().unwrap())
                            as usize;
                    let expected_crc =
                        u32::from_le_bytes(seg_bytes[offset + 4..offset + 8].try_into().unwrap());
                    let frame_total = 8 + payload_len;

                    if offset + frame_total > seg_bytes.len() {
                        break;
                    }

                    let payload_slice = &seg_bytes[offset + 8..offset + frame_total];
                    if crc32fast::hash(payload_slice) == expected_crc {
                        aligned_buf.clear();
                        aligned_buf.extend_from_slice(payload_slice);

                        if let Ok(archived_log) = rkyv::access::<
                            <OpLog as Archive>::Archived,
                            rkyv::rancor::Error,
                        >(&aligned_buf)
                        {
                            let tx_id = archived_log.state.transaction_id.to_native();
                            let agent_id: [u8; 16] = archived_log
                                .agent_id
                                .as_slice()
                                .try_into()
                                .unwrap_or([0; 16]);
                            let ns = archived_log.state.namespace.as_str().to_string();

                            let ptr = FramePointer {
                                tx_id,
                                offset: offset as u64,
                                total_frame_bytes: frame_total as u32,
                            };

                            self.tx_index.insert(tx_id, ptr);
                            self.agent_index.entry(agent_id).or_default().push(ptr);
                            *self.namespace_counts.entry(ns).or_insert(0) += 1;
                            count += 1;

                            if let Some(ref mut f) = idx_file {
                                let _ = f.write_all(&tx_id.to_le_bytes());
                                let _ = f.write_all(&agent_id);
                                let _ = f.write_all(&(offset as u64).to_le_bytes());
                                let _ = f.write_all(&(frame_total as u32).to_le_bytes());
                            }
                        }
                    }
                    offset += frame_total;
                }
                self.cached_record_count.store(count, Ordering::Relaxed);
            }
        }
    }

    /// Performs an O(1) disk seek to `ptr.offset`, reads exactly `ptr.total_frame_bytes`,
    /// verifies the hardware CRC32 checksum, and deserializes that single OpLog.
    fn read_single_frame_at(&self, ptr: FramePointer) -> Result<OpLog, anyhow::Error> {
        let mut file = File::open(&self.history_file_path)?;
        file.seek(SeekFrom::Start(ptr.offset))?;

        let mut frame_buf = vec![0u8; ptr.total_frame_bytes as usize];
        file.read_exact(&mut frame_buf)?;

        if frame_buf.len() < 8 {
            return Err(anyhow::anyhow!("Corrupted frame pointer: length < 8"));
        }

        let payload_len = u32::from_le_bytes(frame_buf[0..4].try_into().unwrap()) as usize;
        let expected_crc = u32::from_le_bytes(frame_buf[4..8].try_into().unwrap());

        if 8 + payload_len > frame_buf.len() {
            return Err(anyhow::anyhow!("Corrupted frame length header"));
        }

        let payload_slice = &frame_buf[8..8 + payload_len];
        if crc32fast::hash(payload_slice) != expected_crc {
            return Err(anyhow::anyhow!(
                "Cold segment CRC32 mismatch at offset {}",
                ptr.offset
            ));
        }

        let mut aligned_buf = rkyv::util::AlignedVec::<16>::with_capacity(payload_len);
        aligned_buf.extend_from_slice(payload_slice);

        let archived =
            rkyv::access::<<OpLog as Archive>::Archived, rkyv::rancor::Error>(&aligned_buf)
                .map_err(|e| anyhow::anyhow!("rkyv validation error: {}", e))?;

        let owned = rkyv::deserialize::<OpLog, rkyv::rancor::Error>(archived)
            .map_err(|e| anyhow::anyhow!("rkyv deserialize error: {}", e))?;

        Ok(owned)
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
        let entry_len = u32::from_le_bytes(mmap[offset..offset + 4].try_into().unwrap()) as usize;
        let expected_crc = u32::from_le_bytes(mmap[offset + 4..offset + 8].try_into().unwrap());
        let frame_total = 8 + entry_len;

        if offset + frame_total > mmap.len() {
            break;
        }

        let entry_slice = &mmap[offset + 8..offset + frame_total];
        if crc32fast::hash(entry_slice) == expected_crc {
            aligned_buf.clear();
            aligned_buf.extend_from_slice(entry_slice);

            if let Ok(archived_batch) =
                rkyv::access::<<Vec<OpLog> as Archive>::Archived, rkyv::rancor::Error>(&aligned_buf)
            {
                callback(archived_batch);
            }
        }
        offset += frame_total;
    }
    Ok(())
}

#[async_trait]
impl ColdStore for SegmentColdStore {
    async fn archive_batch(&self, logs: &[OpLog]) -> Result<usize, anyhow::Error> {
        if logs.is_empty() {
            return Ok(0);
        }

        // Acquire async MutexGuard.
        let _guard = self.write_lock.lock().await;

        let mut seg_file = OpenOptions::new()
            .create(true)
            .read(true)
            .append(true)
            .open(&self.history_file_path)?;

        let mut idx_file = OpenOptions::new()
            .create(true)
            .append(true)
            .open(&self.index_file_path)?;

        let mut current_offset = seg_file.metadata()?.len();
        let mut seg_write_buf = Vec::with_capacity(logs.len() * 256);
        let mut idx_write_buf = Vec::with_capacity(logs.len() * 44);
        let mut new_pointers = Vec::with_capacity(logs.len());

        // Write each OpLog as an individually addressable, CRC32-verified frame
        // so point lookups only read a few hundred bytes instead of a massive batch!
        for log in logs {
            let payload_bytes = to_bytes::<rkyv::rancor::Error>(log)
                .map_err(|e| anyhow::anyhow!("rkyv serialization failed: {}", e))?;

            let payload_len = payload_bytes.len() as u32;
            let crc = crc32fast::hash(&payload_bytes);
            let frame_total = 8 + payload_len;

            seg_write_buf.extend_from_slice(&payload_len.to_le_bytes());
            seg_write_buf.extend_from_slice(&crc.to_le_bytes());
            seg_write_buf.extend_from_slice(&payload_bytes);

            let tx_id = log.state.transaction_id;
            idx_write_buf.extend_from_slice(&tx_id.to_le_bytes());
            idx_write_buf.extend_from_slice(&log.agent_id);
            idx_write_buf.extend_from_slice(&current_offset.to_le_bytes());
            idx_write_buf.extend_from_slice(&frame_total.to_le_bytes());

            let ptr = FramePointer {
                tx_id,
                offset: current_offset,
                total_frame_bytes: frame_total,
            };
            new_pointers.push((log.agent_id, log.state.namespace.clone(), ptr));

            current_offset += frame_total as u64;
        }

        // Single contiguous write + fdatasync for the entire batch
        seg_file.write_all(&seg_write_buf)?;
        seg_file.sync_data()?;

        idx_file.write_all(&idx_write_buf)?;
        idx_file.sync_data()?;

        // Update O(1) in-memory DashMaps after disk durability is confirmed
        for (agent_id, ns, ptr) in new_pointers {
            self.tx_index.insert(ptr.tx_id, ptr);
            self.agent_index.entry(agent_id).or_default().push(ptr);
            *self.namespace_counts.entry(ns).or_insert(0) += 1;
        }

        let count = logs.len();
        self.cached_record_count.fetch_add(count, Ordering::Relaxed);
        Ok(count)
    }

    async fn fetch_historical_timeline(
        &self,
        agent_hex: &str,
    ) -> Result<Vec<TimelineNode>, anyhow::Error> {
        let Ok(target_vec) = hex::decode(agent_hex) else {
            return Ok(Vec::new());
        };
        let Ok(agent_id): Result<[u8; 16], _> = target_vec.try_into() else {
            return Ok(Vec::new());
        };

        let Some(pointers) = self.agent_index.get(&agent_id) else {
            return Ok(Vec::new());
        };

        let mut nodes = Vec::with_capacity(pointers.len());
        for &ptr in pointers.value() {
            if let Ok(log) = self.read_single_frame_at(ptr) {
                let status_str = match log.state.status {
                    AgentStatus::Idle => "IDLE",
                    AgentStatus::Reasoning => "REASONING",
                    AgentStatus::Halted => "HALTED",
                    AgentStatus::ToolExecution => "TOOL_EXEC",
                };
                nodes.push(TimelineNode {
                    tx_id: log.state.transaction_id,
                    timestamp: log.state.timestamp.to_string(),
                    agent_status: status_str.to_string(),
                    payload_preview: log.state.text,
                });
            }
        }
        Ok(nodes)
    }

    async fn fetch_historical_oplogs(
        &self,
        agent_hex: &str,
        min_tx_id: u128,
        max_tx_id: u128,
    ) -> Result<Vec<OpLog>, anyhow::Error> {
        let Ok(target_vec) = hex::decode(agent_hex) else {
            return Ok(Vec::new());
        };
        let Ok(agent_id): Result<[u8; 16], _> = target_vec.try_into() else {
            return Ok(Vec::new());
        };

        let Some(pointers) = self.agent_index.get(&agent_id) else {
            return Ok(Vec::new());
        };

        let mut logs = Vec::new();
        for &ptr in pointers.value() {
            if ptr.tx_id >= min_tx_id && ptr.tx_id <= max_tx_id {
                if let Ok(log) = self.read_single_frame_at(ptr) {
                    logs.push(log);
                }
            }
        }
        Ok(logs)
    }

    async fn fetch_by_txid(&self, target_tx_id: u128) -> Result<Option<OpLog>, anyhow::Error> {
        // O(1) RAM hash lookup -> single disk seek!
        let Some(ptr_ref) = self.tx_index.get(&target_tx_id) else {
            return Ok(None);
        };
        let ptr = *ptr_ref.value();
        let log = self.read_single_frame_at(ptr)?;
        Ok(Some(log))
    }

    async fn get_max_tx_id_for_agent(&self, agent_hex: &str) -> u128 {
        let Ok(target_vec) = hex::decode(agent_hex) else {
            return 0;
        };
        let Ok(agent_id): Result<[u8; 16], _> = target_vec.try_into() else {
            return 0;
        };
        self.agent_index
            .get(&agent_id)
            .and_then(|ptrs| ptrs.iter().map(|p| p.tx_id).max())
            .unwrap_or(0)
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
        let total = self.cached_record_count.load(Ordering::Relaxed);
        if total == 0 {
            return "Empty (0%)".to_string();
        }

        self.namespace_counts
            .iter()
            .max_by_key(|entry| *entry.value())
            .map(|entry| {
                let pct = (*entry.value() as f64 / total as f64) * 100.0;
                format!("{} ({:.1}%)", entry.key(), pct)
            })
            .unwrap_or_else(|| "Empty (0%)".to_string())
    }

    fn engine_name(&self) -> &str {
        "SegmentColdStore (0(1) Indexed rkyv)"
    }
}
