use crate::{
    aegis::QuarantineRecord, axon::MarkleBatch, registry::AgentProcess, EffectRecord, OpLog,
};
use crc32fast::Hasher as Crc32Hasher;
use serde::{Deserialize, Serialize};
use std::{
    collections::HashMap,
    fs::{self, File, OpenOptions},
    io::{Read, Write},
    path::Path,
};

/// Snapshot representation of a single namespace's active Merkle buffer
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ActiveTreeBufferSnapshot {
    pub namespace: String,
    pub current_batch_id: u64,
    pub parent_batch_root: [u8; 32],
    pub accumulated_leaves: Vec<[u8; 32]>,
    pub accumulated_logs: Vec<OpLog>,
    pub accumulated_tx_ids: Vec<u128>,
}

/// Consolidated state of the Axon Merkle DAG at checkpoint time
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AxonCheckpointState {
    pub global_batch_counter: u64,
    pub active_buffers: Vec<ActiveTreeBufferSnapshot>,
    pub batch_archive: Vec<MarkleBatch>,
}

/// The unified atomic state checkpoint captured during WAL compaction
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StateCheckpoint {
    pub version: u32,
    pub timestamp: i64,
    pub last_tx_id: u128,
    /// Loro CRDT binary snapshots keyed by namespace
    pub crdt_snapshots: HashMap<String, Vec<u8>>,
    /// Cryptographic Merkle DAG state
    pub axon_state: AxonCheckpointState,
    /// Deterministic $0 replay cache
    pub effects: Vec<EffectRecord>,
    /// Active quarantine blocklist
    pub quarantines: Vec<QuarantineRecord>,
    /// Live agent process table
    pub active_agents: Vec<AgentProcess>,
}

/// Discrete control mutations recorded in the control journal between compactions
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum ControlMutation {
    Quarantine(QuarantineRecord),
    LiftQuarantine { agent_hex: String },
    RecordEffect(EffectRecord),
    TouchAgent(AgentProcess),
}

pub struct CheckpointEngine;

impl CheckpointEngine {
    /// Atomically persists a StateCheckpoint to disk using tempfile + rename
    pub fn write_checkpoint_atomically(
        path: &Path,
        checkpoint: &StateCheckpoint,
    ) -> Result<(), anyhow::Error> {
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent)?;
        }

        let temp_path = path.with_extension("tmp");
        let payload_bytes = postcard::to_allocvec(checkpoint)
            .map_err(|e| anyhow::anyhow!("Failed to serialize checkpoint with postcard: {}", e))?;

        let mut hasher = Crc32Hasher::new();
        hasher.update(&payload_bytes);
        let checksum = hasher.finalize();

        let mut file = File::create(&temp_path)?;
        // Write 4B length + 4B CRC32 + payload
        let len_bytes = (payload_bytes.len() as u32).to_le_bytes();
        let crc_bytes = checksum.to_le_bytes();

        file.write_all(&len_bytes)?;
        file.write_all(&crc_bytes)?;
        file.write_all(&payload_bytes)?;
        file.sync_all()?;

        fs::rename(&temp_path, path)?;
        Ok(())
    }

    /// Loads a StateCheckpoint from disk with CRC32 verification
    pub fn load_checkpoint(path: &Path) -> Option<StateCheckpoint> {
        if !path.exists() {
            return None;
        }

        let mut file = File::open(path).ok()?;
        let mut len_buf = [0u8; 4];
        let mut crc_buf = [0u8; 4];

        file.read_exact(&mut len_buf).ok()?;
        file.read_exact(&mut crc_buf).ok()?;

        let payload_len = u32::from_le_bytes(len_buf) as usize;
        let expected_crc = u32::from_le_bytes(crc_buf);

        let mut payload_bytes = vec![0u8; payload_len];
        file.read_exact(&mut payload_bytes).ok()?;

        let mut hasher = Crc32Hasher::new();
        hasher.update(&payload_bytes);
        let actual_crc = hasher.finalize();

        if actual_crc != expected_crc {
            eprintln!(
                "[CHECKPOINT ERROR] CRC32 mismatch in '{}'! Expected {:x}, got {:x}. Aborting checkpoint load.",
                path.display(),
                expected_crc,
                actual_crc
            );
            return None;
        }

        postcard::from_bytes(&payload_bytes)
            .map_err(|e| {
                eprintln!("[CHECKPOINT ERROR] Deserialization failed: {}", e);
                e
            })
            .ok()
    }

    /// Appends a discrete control mutation with CRC32 framing to the control journal
    pub fn append_control_mutation(
        journal_path: &Path,
        mutation: &ControlMutation,
    ) -> Result<(), anyhow::Error> {
        if let Some(parent) = journal_path.parent() {
            fs::create_dir_all(parent)?;
        }

        let payload_bytes = postcard::to_allocvec(mutation)
            .map_err(|e| anyhow::anyhow!("Failed to serialize control mutation: {}", e))?;

        let mut hasher = Crc32Hasher::new();
        hasher.update(&payload_bytes);
        let checksum = hasher.finalize();

        let len_bytes = (payload_bytes.len() as u32).to_le_bytes();
        let crc_bytes = checksum.to_le_bytes();

        let mut file = OpenOptions::new()
            .create(true)
            .append(true)
            .open(journal_path)?;

        file.write_all(&len_bytes)?;
        file.write_all(&crc_bytes)?;
        file.write_all(&payload_bytes)?;
        file.sync_data()?;

        Ok(())
    }

    /// Replays all valid control mutations from the journal
    pub fn replay_control_journal(journal_path: &Path) -> Vec<ControlMutation> {
        if !journal_path.exists() {
            return Vec::new();
        }

        let mut file = match File::open(journal_path) {
            Ok(f) => f,
            Err(_) => return Vec::new(),
        };

        let mut mutations = Vec::new();
        let mut len_buf = [0u8; 4];
        let mut crc_buf = [0u8; 4];

        loop {
            if file.read_exact(&mut len_buf).is_err() {
                break;
            }
            if file.read_exact(&mut crc_buf).is_err() {
                eprintln!("[CONTROL JOURNAL WARN] Truncated header in journal. Halting replay.");
                break;
            }

            let entry_len = u32::from_le_bytes(len_buf) as usize;
            let expected_crc = u32::from_le_bytes(crc_buf);

            let mut payload = vec![0u8; entry_len];
            if file.read_exact(&mut payload).is_err() {
                eprintln!("[CONTROL JOURNAL WARN] Truncated payload in journal. Halting replay.");
                break;
            }

            let mut hasher = Crc32Hasher::new();
            hasher.update(&payload);
            if hasher.finalize() != expected_crc {
                eprintln!("[CONTROL JOURNAL CORRUPTION] CRC32 mismatch in journal entry. Skipping.");
                continue;
            }

            if let Ok(mutation) = postcard::from_bytes::<ControlMutation>(&payload) {
                mutations.push(mutation);
            }
        }

        mutations
    }

    /// Truncates the control journal after a successful checkpoint is committed
    pub fn truncate_control_journal(journal_path: &Path) {
        if journal_path.exists() {
            let _ = fs::remove_file(journal_path);
        }
    }
}

