use dashmap::DashMap;
use memmap2::MmapOptions;
use rkyv::{Archive, Archived};
use std::format;
use std::println;

use std::u64;
use std::{fs::File, sync::Arc};
use tokio::sync::broadcast::Sender;

use crate::api::ForkConfig;
use crate::checkpoint::{CheckpointEngine, ControlMutation};
use crate::cold_store::ColdStore;
use crate::merkle::MerkleEngine;
use crate::EffectKey;
use crate::ExecutionStage;
use crate::{config::RaqimConfig, wal::WalEngine, ExecutionRecord, SystemEvent};
use serde::Serialize;

pub enum RebuildMode {
    Resurrection,
    TimeTravel(u64), //
}

pub struct EffectStore {
    config: Arc<RaqimConfig>,
    merkle: Arc<MerkleEngine>,
    cold_store: Arc<dyn ColdStore>,
    wal_engine: Arc<WalEngine>,
    event_tx: Sender<SystemEvent>,
    /// Completed `Outcome` records indexed for 0(1) $0.00 deterministitc replay.
    effect_index: DashMap<EffectKey, ExecutionRecord>,

    /// Pre-execution `Intents` records that have not yet received a matching `Outcome`
    pending_intents: DashMap<EffectKey, ExecutionRecord>,
}

#[derive(Clone, Debug, Serialize)]
pub struct UnifiedSearchResult {
    pub tx_id: u128,
    pub agent_hex: String,
    pub namespace: String,
    pub text: String,
    pub timestamp: i64,
    pub score: f32,
    pub source: &'static str,
}

impl EffectStore {
    pub fn new(
        config: Arc<RaqimConfig>,
        merkle: Arc<MerkleEngine>,
        cold_store: Arc<dyn ColdStore>,
        wal_engine: Arc<WalEngine>,
        event_tx: Sender<SystemEvent>,
    ) -> Self {
        Self {
            config,
            merkle,
            cold_store,
            wal_engine,
            event_tx,
            effect_index: DashMap::new(),
            pending_intents: DashMap::new(),
        }
    }

    /// STATIC ZERO-COPY SCANNER: Scans any WAL segment with 8-byte frame header validation
    pub fn scan_wal_file<F>(wal_path: &str, mut callback: F) -> Result<(), anyhow::Error>
    where
        F: FnMut(&Archived<ExecutionRecord>),
    {
        let file = match File::open(wal_path) {
            Ok(f) => f,
            Err(_) => return Ok(()),
        };

        let mmap = unsafe { MmapOptions::new().map(&file)? };
        let mut offset = 0;
        let mut aligned_buf = rkyv::util::AlignedVec::<16>::new();

        while offset + 8 <= mmap.len() {
            let entry_len =
                u32::from_le_bytes(mmap[offset..offset + 4].try_into().unwrap()) as usize;
            let expected_crc = u32::from_le_bytes(mmap[offset + 4..offset + 8].try_into().unwrap());
            let frame_total = 8 + entry_len;

            if offset + frame_total > mmap.len() {
                break;
            }

            let entry_slice = &mmap[offset + 8..offset + frame_total];
            if crc32fast::hash(entry_slice) != expected_crc {
                offset += frame_total;
                continue;
            }

            aligned_buf.clear();
            aligned_buf.extend_from_slice(entry_slice);

            if let Ok(archived_batch) = rkyv::access::<
                <Vec<ExecutionRecord> as Archive>::Archived,
                rkyv::rancor::Error,
            >(&aligned_buf)
            {
                for rec in archived_batch.as_slice() {
                    callback(rec);
                }
            }

            offset += frame_total;
        }

        Ok(())
    }

    pub fn scan_wal_file_until<F>(wal_path: &str, mut callback: F) -> Result<(), anyhow::Error>
    where
        F: FnMut(&Archived<ExecutionRecord>) -> bool,
    {
        let file = match File::open(wal_path) {
            Ok(f) => f,
            Err(_) => return Ok(()),
        };
        let mmap = unsafe { MmapOptions::new().map(&file)? };
        let mut offset = 0;
        let mut aligned_buf = rkyv::util::AlignedVec::<16>::new();

        while offset + 8 <= mmap.len() {
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
                    <Vec<ExecutionRecord> as Archive>::Archived,
                    rkyv::rancor::Error,
                >(&aligned_buf)
                {
                    for log in archived_batch.as_slice() {
                        if !callback(log) {
                            return Ok(());
                        }
                    }
                }
            }
            offset += frame_total;
        }
        Ok(())
    }

    /// THE RESURRECTION ENGINE
    /// Rebuild the agent's causal Oplog titmeline from ColdStore + Hot WAL
    pub async fn rebuild_agent_timeline(
        &self,
        agent_hex: &str,
        target_tx_id: u128,
        wal_engine: Arc<WalEngine>,
    ) -> Result<Vec<ExecutionRecord>, anyhow::Error> {
        // Find the highest known tx_id for this agent.
        let actual_target_tx = if target_tx_id == u128::MAX {
            // Checking the WAL Index first
            let wal_max = {
                let idx = wal_engine.index.read().await;
                idx.keys().copied().filter(|&k| k > 0).max()
            };

            if let Some(max_tx) = wal_max {
                max_tx
            } else {
                self.cold_store.get_max_tx_id_for_agent(agent_hex).await
            }
        } else {
            target_tx_id
        };

        // 1. Fetch compacted historical OpLogs from O(1) indexed ColdStore
        let mut historical_records = self
            .cold_store
            .fetch_historical_records(agent_hex, 0, actual_target_tx)
            .await
            .unwrap_or_default();

        // 2. Append any uncompacted trailing OpLogs from the hot WAL
        let target_hex_owned = agent_hex.to_string();
        let _ = Self::scan_wal_file(&self.config.wal_path, |archived_record| {
            let tx = archived_record.tx_id.to_native();
            if tx <= actual_target_tx {
                let record_agent_hex = hex::encode(archived_record.agent_id.as_slice());
                if record_agent_hex == target_hex_owned {
                    if let Ok(owned_record) =
                        rkyv::deserialize::<ExecutionRecord, rkyv::rancor::Error>(archived_record)
                    {
                        historical_records.push(owned_record);
                    }
                }
            }
        });

        // Deduplicate and order monotonically by UUIDv7 transaction_id
        historical_records.sort_by_key(|l| l.tx_id);
        historical_records.dedup_by_key(|l| l.tx_id);

        Ok(historical_records)
    }

    /// FORENSIC POINT LOOKUP: Returns the typed ExecutionRecord struct from Hot WAL or O(1) ColdStore
    pub async fn fetch_by_txid(
        &self,
        target_tx_id: u128,
    ) -> Result<ExecutionRecord, anyhow::Error> {
        let mut hot_found: Option<ExecutionRecord> = None;

        Self::scan_wal_file_until(&self.config.wal_path, |archived| {
            if archived.tx_id.to_native() == target_tx_id {
                if let Ok(owned) =
                    rkyv::deserialize::<ExecutionRecord, rkyv::rancor::Error>(archived)
                {
                    hot_found = Some(owned);
                    return false;
                }
            }

            true // keep scanning
        })?;

        if let Some(log) = hot_found {
            return Ok(log);
        }

        if let Some(cold_log) = self.cold_store.fetch_by_txid(target_tx_id).await? {
            return Ok(cold_log);
        }

        Err(anyhow::anyhow!(
            "Transaction ID {:032x} not found in Hot WAL or ColdStore",
            target_tx_id
        ))
    }

    /// The Unified Engine for both Resurrection (Live) and Time Travel (Isolated)
    pub async fn boot_historical_agent(
        &self,
        agent_hex: &str,
        target_tx_id: Option<u128>,
        fork_config: Option<ForkConfig>,
        is_isolated_debug: bool,
    ) -> Result<(), anyhow::Error> {
        let fetch_target = target_tx_id.unwrap_or(u128::MAX);

        let historical_oplog = self
            .rebuild_agent_timeline(agent_hex, fetch_target, self.wal_engine.clone())
            .await?;

        if historical_oplog.is_empty() {
            return Err(anyhow::anyhow!(
                "CRITICAL: Agent {} has no immutable memory on disk. Rebuild aborted.",
                agent_hex
            ));
        }

        let original_bytes =
            hex::decode(agent_hex).map_err(|e| anyhow::anyhow!("Invalid agent hex: {}", e))?;
        let mut phantom_bytes = [0u8; 16];
        if original_bytes.len() == 16 {
            phantom_bytes.copy_from_slice(&original_bytes);
        }

        if is_isolated_debug {
            let tx_id_bytes = target_tx_id.unwrap_or(0).to_be_bytes();
            let mut salt = [0u8; 16];
            salt[0..8].copy_from_slice(&tx_id_bytes[0..8]);
            salt[8..16].copy_from_slice(&tx_id_bytes[8..16]);

            for i in 0..16 {
                phantom_bytes[i] ^= salt[i];
                phantom_bytes[i] ^= 0xFF;
            }
        }

        let sandbox_agent_hex = hex::encode(phantom_bytes);

        if let Some(fork) = &fork_config {
            println!(
                "[TIME MACHINE] Applied {} reality fork overrides",
                fork.env_overrides.len()
            );
        }

        Ok(())
    }

    /// Write-Ahead Intent / Denial Recorder: Seals a pre-execution `Intent` (or `Denied` interdicion ) into the WAL & Merkle DAG BEFORE any external tool or function body runs.
    pub async fn record_intent_or_denial(
        &self,
        agent_id: [u8; 16],
        step_ordinal: u64,
        call_signature_hash: [u8; 32],
        namespace: &str,
        allowed: bool,
        reason_or_preview: Vec<u8>,
    ) -> Result<u128, anyhow::Error> {
        let stage = if allowed {
            ExecutionStage::Intent
        } else {
            ExecutionStage::Denied
        };

        let unsealed = ExecutionRecord::new(
            agent_id,
            step_ordinal,
            reason_or_preview,
            stage,
            namespace.to_string(),
            call_signature_hash,
        );

        let (sealed_record, optional_batch) = self.merkle.seal_record(unsealed);
        let tx_id = sealed_record.tx_id;

        if allowed {
            let key = EffectKey::derive(&agent_id, step_ordinal, &call_signature_hash);
            self.pending_intents.insert(key, sealed_record.clone());
        }

        if let Some(batch) = optional_batch {
            let _ = self
                .event_tx
                .send(SystemEvent::MarkleBatchCrystallized { batch });
        }

        self.wal_engine.append(sealed_record).await?;
        Ok(tx_id)
    }

    /// Post-Execution Outcome Recorder: Resolves any pending `Intent`, seals the `Outcome` into MErkle DAG & WAL and caches the output in `effect_idx` for 0(1) $0.00 replay    
    pub async fn record_effect(
        &self,
        agent_id: [u8; 16],
        step_ordinal: u64,
        call_signature_hash: [u8; 32],
        output_payload: Vec<u8>,
        namespace: &str,
    ) -> Result<u128, anyhow::Error> {
        let unsealed = ExecutionRecord::new(
            agent_id,
            step_ordinal,
            output_payload,
            ExecutionStage::Outcome,
            namespace.to_string(),
            call_signature_hash,
        );

        let (sealed_record, optional_batch) = self.merkle.seal_record(unsealed);
        let tx_id = sealed_record.tx_id;

        // Calculate RAM Lookup Key
        let effect_key = EffectKey::derive(&agent_id, step_ordinal, &call_signature_hash);

        // Resolve the write-ahead intent (no longer in-doubt!)
        self.pending_intents.remove(&effect_key);
        self.effect_index.insert(effect_key, sealed_record.clone());

        let _ = CheckpointEngine::append_control_mutation(
            std::path::Path::new(&self.config.control_journal_path),
            &ControlMutation::RecordEffect(sealed_record),
        );

        if let Some(batch) = optional_batch {
            let _ = self
                .event_tx
                .send(SystemEvent::MarkleBatchCrystallized { batch });
        }

        self.wal_engine.append(sealed_record).await?;

        Ok(tx_id)
    }

    /// Get Effect (Replay mode): Perform 0(1) Ram lookup to fetch recorded payload
    pub fn get_effect(
        &self,
        agent_id: &[u8; 16],
        step_ordinal: u64,
        call_signature_hash: &[u8; 32],
    ) -> Option<ExecutionRecord> {
        let record_key = EffectKey::derive(agent_id, step_ordinal, call_signature_hash);

        // O(1) sharded RAM lookup
        if let Some(record) = self.effect_index.get(&record_key) {
            return Some(record.value().clone());
        }

        None
    }

    /// Returns all write-ahead `Intent` records that never received a matching `Outcome` (e.g., interrupted mid-execution by a process crash or network failure).
    pub fn get_in_doubt_intents(&self) -> Vec<ExecutionRecord> {
        self.pending_intents
            .iter()
            .map(|e| e.value().clone())
            .collect()
    }

    /// Exports all cached effect records for checkpointing
    pub fn export_effects(&self) -> Vec<ExecutionRecord> {
        self.effect_index
            .iter()
            .map(|e| e.value().clone())
            .collect()
    }

    /// Hydrates effect records from checkpoint
    pub fn hydrate_effects(&self, effects: Vec<ExecutionRecord>) {
        for record in effects {
            let key = EffectKey::derive(
                &record.agent_id,
                record.step_ordinal,
                &record.call_signature_hash,
            );
            self.effect_index.insert(key, record);
        }
    }

    /// Directly records an effect without re-journaling (used during replay/hydration)
    pub fn record_effect_direct(&self, record: ExecutionRecord) {
        let key = EffectKey::derive(
            &record.agent_id,
            record.step_ordinal,
            &record.call_signature_hash,
        );
        self.effect_index.insert(key, record);
    }

    /// Used during WAL crash recovery to reconstruct both `pending_intents` (In-Doubt) and `effect_index` (Outcomes).
    pub fn hydrate_record_from_wal(&self, record: ExecutionRecord) {
        let key = EffectKey::derive(
            &record.agent_id,
            record.step_ordinal,
            &record.call_signature_hash,
        );

        match record.stage {
            ExecutionStage::Intent => {
                if !self.effect_index.contains_key(&key) {
                    self.pending_intents.insert(key, record);
                }
            }
            ExecutionStage::Denied => {
                self.pending_intents.remove(&key);
                self.effect_index.insert(key, record);
            }

            _ => {}
        }
    }
}
