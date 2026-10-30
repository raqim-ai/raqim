use dashmap::DashMap;
use memmap2::MmapOptions;
use rkyv::{Archive, Archived};

use std::u64;
use std::{fs::File, sync::Arc};
use tokio::sync::broadcast::Sender;

use crate::checkpoint::{CheckpointEngine, ControlMutation};
use crate::cold_store::ColdStore;
use crate::merkle::MerkleEngine;
use crate::EffectKey;
use crate::ExecutionStage;
use crate::{config::RaqimConfig, wal::WalEngine, ExecutionRecord, SystemEvent};

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

    /// Write-Ahead intent / Denial Recorder:
    /// 1. Prepares the leaf hash.
    /// 2. Persists to WAL (`fdatasync`) FIRST.
    /// 3. Commits to `MerkleEngine` and `pending_intents` ONLY after disk durability succeeds.
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

        let prepared = self.merkle.prepare_record(unsealed);
        let tx_id = prepared.tx_id;

        // DURABILITY FIRST
        self.wal_engine.append(prepared.clone()).await?;

        // Now update RAM
        if allowed {
            let key = EffectKey::derive(&agent_id, step_ordinal, &call_signature_hash);
            self.pending_intents.insert(key, prepared.clone());
        }

        if let Some((batch, records)) = self.merkle.commit_prepared_record(prepared) {
            let _ = self
                .event_tx
                .send(SystemEvent::MerkleBatchCrystallized { batch, records });
        }

        Ok(tx_id)
    }

    /// Post-Execution Outcome Recorder:
    /// 1. Prepares the leaf hash.
    /// 2. Persists to the WAL (`fdatasync`) FIRST.
    /// 3. Resolves `pending_intents` updates `effect_index`, and commits to `MerkleEngine`.       
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

        let prepared = self.merkle.prepare_record(unsealed);
        let tx_id = prepared.tx_id;

        // 1. DURABILITY FIRST
        self.wal_engine.append(prepared.clone()).await?;

        let _ = CheckpointEngine::append_control_mutation(
            std::path::Path::new(&self.config.control_journal_path),
            &ControlMutation::RecordEffect(prepared.clone()),
        );

        // Now update the RAM state
        let effect_key = EffectKey::derive(&agent_id, step_ordinal, &call_signature_hash);
        self.pending_intents.remove(&effect_key);
        self.effect_index.insert(effect_key, prepared.clone());

        if let Some((batch, records)) = self.merkle.commit_prepared_record(prepared) {
            let _ = self
                .event_tx
                .send(SystemEvent::MerkleBatchCrystallized { batch, records });
        }

        Ok(tx_id)
    }

    /// NDPA / GDPR Right-to-Erasure: Scrub `payload_bytes` and `payload_salt` in RAM and ColdStore
    /// while preserving `payload_hash` and `leaf_hash` so Merkle inclucion proofs stay 100% valid.
    pub async fn redact_record(&self, target_tx_id: u128) -> Result<bool, anyhow::Error> {
        let mut found = false;

        for mut entry in self.effect_index.iter_mut() {
            if entry.value().tx_id == target_tx_id {
                entry.value_mut().redact_pii();
                found = true;
            }
        }

        for entry in self.merkle.active_buffers.iter() {
            let mut buf = entry.value().write();
            for rec in buf.accumulated_records.iter_mut() {
                if rec.tx_id == target_tx_id {
                    rec.redact_pii();
                    found = true;
                }
            }
        }

        if self.cold_store.redact_by_txid(target_tx_id).await? {
            found = true;
        }

        Ok(found)
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
        self.effect_index
            .get(&record_key)
            .map(|r| r.value().clone())
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
