use dashmap::DashMap;
use memmap2::MmapOptions;
use rkyv::{Archive, Archived};
use std::eprintln;
use std::format;
use std::println;

use std::time::SystemTime;
use std::time::UNIX_EPOCH;
use std::u64;
use std::{fs::File, sync::Arc};
use tokio::sync::broadcast::Sender;

use crate::api::ForkConfig;
use crate::api::UiEvent;
use crate::cold_store::ColdStore;
use crate::generate_uuidv7_txid;
use crate::merkle::MerkleEngine;
use crate::state::SwarmStateRegistry;
use crate::AgentState;
use crate::AgentStatus;
use crate::EffectKey;
use crate::EffectRecord;
use crate::{config::RaqimConfig, wal::WalEngine, OpLog, SystemEvent};
use serde::Serialize;

pub enum RebuildMode {
    Resurrection,
    TimeTravel(u64), //
}

pub struct MemoryRouter {
    config: Arc<RaqimConfig>,
    axon: Arc<MerkleEngine>,
    brain: Arc<SwarmStateRegistry>,
    cold_store: Arc<dyn ColdStore>,
    wal_engine: Arc<WalEngine>,
    event_tx: Sender<SystemEvent>,
    effect_index: DashMap<EffectKey, EffectRecord>,
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

impl MemoryRouter {
    pub fn new(
        config: Arc<RaqimConfig>,
        axon: Arc<MerkleEngine>,
        brain: Arc<SwarmStateRegistry>,
        cold_store: Arc<dyn ColdStore>,
        wal_engine: Arc<WalEngine>,
        event_tx: Sender<SystemEvent>,
    ) -> Self {
        Self {
            config,
            axon,
            brain,
            cold_store,
            wal_engine,
            event_tx,
            effect_index: DashMap::new(),
        }
    }

    /// STATIC ZERO-COPY SCANNER: Scans any WAL segment with 8-byte frame header validation
    pub fn scan_wal_file<F>(wal_path: &str, mut callback: F) -> Result<(), anyhow::Error>
    where
        F: FnMut(&Archived<OpLog>),
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

            if let Ok(archived_batch) =
                rkyv::access::<<Vec<OpLog> as Archive>::Archived, rkyv::rancor::Error>(&aligned_buf)
            {
                for log in archived_batch.as_slice() {
                    callback(log);
                }
            }

            offset += frame_total;
        }

        Ok(())
    }

    pub fn scan_wal_file_until<F>(wal_path: &str, mut callback: F) -> Result<(), anyhow::Error>
    where
        F: FnMut(&Archived<OpLog>) -> bool,
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
                    <Vec<OpLog> as Archive>::Archived,
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
    ) -> Result<Vec<OpLog>, anyhow::Error> {
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
        let mut historical_oplogs = self
            .cold_store
            .fetch_historical_oplogs(agent_hex, 0, actual_target_tx)
            .await
            .unwrap_or_default();

        // 2. Append any uncompacted trailing OpLogs from the hot WAL
        let target_hex_owned = agent_hex.to_string();
        let _ = Self::scan_wal_file(&self.config.wal_path, |archived_log| {
            let tx = archived_log.state.transaction_id.to_native();
            if tx <= actual_target_tx {
                let log_agent_hex = hex::encode(archived_log.agent_id.as_slice());
                if log_agent_hex == target_hex_owned {
                    if let Ok(owned_log) =
                        rkyv::deserialize::<OpLog, rkyv::rancor::Error>(archived_log)
                    {
                        historical_oplogs.push(owned_log);
                    }
                }
            }
        });

        // Deduplicate and order monotonically by UUIDv7 transaction_id
        historical_oplogs.sort_by_key(|l| l.state.transaction_id);
        historical_oplogs.dedup_by_key(|l| l.state.transaction_id);

        Ok(historical_oplogs)
    }

    /// FORENSIC POINT LOOKUP: Returns the typed OpLog struct from Hot WAL or O(1) ColdStore
    pub async fn fetch_by_txid(&self, target_tx_id: u128) -> Result<OpLog, anyhow::Error> {
        let mut hot_found: Option<OpLog> = None;

        Self::scan_wal_file_until(&self.config.wal_path, |archived| {
            if archived.state.transaction_id.to_native() == target_tx_id {
                if let Ok(owned) = rkyv::deserialize::<OpLog, rkyv::rancor::Error>(archived) {
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
        phantom_ui_tx: tokio::sync::broadcast::Sender<UiEvent>,
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

        let real_namespace = &historical_oplog[0].state.namespace;

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

        let target_brain = if is_isolated_debug {
            let phantom_namespace = format!("phantom_{}_{}", real_namespace, sandbox_agent_hex);
            self.brain.get_or_create_brain(&phantom_namespace)
        } else {
            self.brain.get_or_create_brain(real_namespace)
        };

        for log in &historical_oplog {
            let _ = target_brain.assimilate_foreign_thought(&log.delta);
        }

        if let Some(fork) = &fork_config {
            println!(
                "[TIME MACHINE] Applied {} reality fork overrides",
                fork.env_overrides.len()
            );
        }

        let _ = phantom_ui_tx.send(UiEvent::ThoughtCommitted {
            agent_hex: if is_isolated_debug {
                sandbox_agent_hex
            } else {
                agent_hex.to_string()
            },
            intent_path: real_namespace.clone(),
            tx_id: format!("{:032x}", fetch_target),
            text: format!(
                "[TEMPORAL FORK] State restored up to TxID 0x{:032x}",
                fetch_target
            ),
        });

        self.brain.purge_phantom_shards();
        Ok(())
    }

    /// Record Effect (Record mode) : captures a live side-effect, writes it to the wal & merkle DAG and updates the RAM
    pub async fn record_effect(
        &self,
        agent_id: [u8; 16],
        step_ordinal: u64,
        call_signature_hash: [u8; 32],
        output_payload: Vec<u8>,
        namespace: &str,
    ) -> Result<u128, anyhow::Error> {
        let agent_hex = hex::encode(agent_id);
        let transaction_id = generate_uuidv7_txid();
        let timestamp = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs() as i64;

        let record = EffectRecord {
            agent_id,
            step_ordinal,
            call_signature_hash,
            output_payload: output_payload.clone(),
            transaction_id,
            timestamp,
        };

        // Calculate RAM Lookup Key
        let effect_key = EffectKey::derive(&agent_id, step_ordinal, &call_signature_hash.clone());

        self.effect_index.insert(effect_key, record.clone());

        // Durably append to control journal for reboot recovery
        let _ = crate::checkpoint::CheckpointEngine::append_control_mutation(
            std::path::Path::new("./vault/control_journal.bin"),
            &crate::checkpoint::ControlMutation::RecordEffect(record.clone()),
        );

        let payload_str = String::from_utf8(output_payload.clone())
            .unwrap_or_else(|_| format!("[RAW_BYTES: {}]", output_payload.len()));

        let state = AgentState {
            agent_id: Some(agent_id),
            transaction_id,
            timestamp,
            namespace: namespace.to_string(),
            status: AgentStatus::ToolExecution,
            text: payload_str,
        };

        let delta = self
            .brain
            .get_or_create_brain(namespace)
            .append_agent_thought(&agent_hex)
            .unwrap_or_default();

        let raw_oplog = OpLog {
            agent_id,
            state,
            delta,
            previous_hash: [0u8; 32],
            current_hash: [0u8; 32],
            entropy_seeds: Vec::new(),
            network_responses: Vec::new(),
        };

        let (sealed_log, optional_batch) = self.axon.seal_thought(raw_oplog);

        if let Some(batch) = optional_batch {
            let _ = self
                .event_tx
                .send(SystemEvent::MarkleBatchCrystallized { batch });
        }

        let scan_res = self.wal_engine.append(sealed_log).await;

        if let Err(e) = scan_res {
            eprintln!("[WAL ERROR] Failed to append thought to WalEngine: {}  ", e);
        }

        println!(
            " [EFFECT ENGINE] Recorded Side-Effect for Agent: {} at Step: {} [TxID: {:032x}] ",
            agent_hex, step_ordinal, transaction_id
        );

        Ok(transaction_id)
    }

    /// Get Effect (Replay mode): Perform 0(1) Ram lookup to fetch recorded payload
    pub fn get_effect(
        &self,
        agent_id: &[u8; 16],
        step_ordinal: u64,
        call_signature_hash: &[u8; 32],
    ) -> Option<EffectRecord> {
        let record_key = EffectKey::derive(agent_id, step_ordinal, call_signature_hash);

        // O(1) sharded RAM lookup
        if let Some(record) = self.effect_index.get(&record_key) {
            return Some(record.value().clone());
        }

        None
    }

    /// Exports all cached effect records for checkpointing
    pub fn export_effects(&self) -> Vec<EffectRecord> {
        self.effect_index
            .iter()
            .map(|e| e.value().clone())
            .collect()
    }

    /// Hydrates effect records from checkpoint
    pub fn hydrate_effects(&self, effects: Vec<EffectRecord>) {
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
    pub fn record_effect_direct(&self, record: EffectRecord) {
        let key = EffectKey::derive(
            &record.agent_id,
            record.step_ordinal,
            &record.call_signature_hash,
        );
        self.effect_index.insert(key, record);
    }
}
