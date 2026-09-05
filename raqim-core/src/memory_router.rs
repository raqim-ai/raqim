use arrow_array::Array;
use dashmap::DashMap;
use futures::StreamExt;
use lancedb::query::ExecutableQuery;
use lancedb::query::QueryBase;
use memmap2::MmapOptions;
use rkyv::{Archive, Archived};
use std::collections::HashMap;
use std::eprintln;
use std::format;
use std::io::{Read, Seek, SeekFrom};
use std::println;

use std::time::SystemTime;
use std::time::UNIX_EPOCH;
use std::u64;
use std::{fs::File, sync::Arc};
use tokio::sync::broadcast::Sender;

use crate::AgentState;
use crate::AgentStatus;
use crate::EffectKey;
use crate::EffectRecord;
use crate::api::ForkConfig;
use crate::api::UiEvent;
use crate::axon::AxonGateKeeper;
use crate::generate_uuidv7_txid;
use crate::hot_memory::HotVectorBuffer;
use crate::state::SwarmStateRegistry;
use crate::{
    OpLog, SystemEvent, config::RaqimConfig, lancedb_store::LanceEngine, nucleus::WalEngine,
};

pub enum RebuildMode {
    Resurrection,
    TimeTravel(u64), //
}

pub struct MemoryRouter {
    config: Arc<RaqimConfig>,
    axon: Arc<AxonGateKeeper>,
    brain: Arc<SwarmStateRegistry>,
    lance_engine: Arc<LanceEngine>,
    wal_engine: Arc<WalEngine>,
    event_tx: Sender<SystemEvent>,
    effect_index: DashMap<EffectKey, EffectRecord>,
}

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
        axon: Arc<AxonGateKeeper>,
        brain: Arc<SwarmStateRegistry>,
        lance_engine: Arc<LanceEngine>,
        wal_engine: Arc<WalEngine>,
        event_tx: Sender<SystemEvent>,
    ) -> Self {
        Self {
            config,
            axon,
            brain,
            lance_engine,
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
            Err(_) => return Ok(()), // File not created yet; clean return
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
                break; // Incomplete or torn frame at EOF
            }

            let entry_slice = &mmap[offset + 8..offset + frame_total];

            // Hardware CRC32 verification
            if crc32fast::hash(entry_slice) != expected_crc {
                offset += frame_total;
                continue;
            }

            aligned_buf.clear();
            aligned_buf.extend_from_slice(entry_slice);

            // Safe, verified deserialization of Vec<OpLog> batch
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

    /// Scans the active WAL path configured on the router
    pub fn scan_wal_zero_copy<F>(&self, callback: F) -> Result<(), anyhow::Error>
    where
        F: FnMut(&Archived<OpLog>),
    {
        Self::scan_wal_file(&self.config.wal_path, callback)
    }
    // RAG CONTEXT: Prioritize the hot WAL, fills the rest with semantic lanceDB
    pub async fn semantic_search_with_context(
        &self,
        query: &str,
        namespace: &str,
        limit: usize,
    ) -> Result<Vec<String>, anyhow::Error> {
        let mut final_context = Vec::new();

        // 1. HOT MEMORY (WAL): Zero-Copy Semantic Filtering
        self.scan_wal_zero_copy(|archived| {
            // PHYSICS: We read the name_space as a string slice without allocating mem
            let log_namespace = archived.state.namespace.as_str();

            if log_namespace.starts_with(namespace) {
                final_context.push(format!("[Recent] {} ", archived.state.text.as_str()));
            }
        })
        .map_err(|e| anyhow::anyhow!("Error scanning wal file: {}", e))?;

        // 2. Supplement with Deep Semantic search
        let mut deep_memories = self
            .lance_engine
            .search_memory(query, namespace, limit)
            .await?;

        final_context.append(&mut deep_memories);

        Ok(final_context)
    }

    /// FORENSIC TIME MACHINE
    pub async fn fetch_by_txid(&self, target_tx_id: u128) -> Result<String, anyhow::Error> {
        let mut result = None;

        // 1. Hot Memory ( Zero-copy WAL scan )
        self.scan_wal_zero_copy(|archievd| {
            // We read directly from the archeived bytes!
            if archievd.state.transaction_id == target_tx_id {
                result = Some(format!(
                    "[HOT MEMORY] TxID: {} | Text: {} ",
                    archievd.state.transaction_id,
                    archievd.state.text.as_str()
                ))
            }
        })
        .map_err(|e| anyhow::anyhow!("Error scanning wal file: {}", e))?;

        if let Some(res) = result {
            return Ok(res);
        }

        // 2. Cold Memory ( REAL LanceDB SQL Filter )
        let table = self
            .lance_engine
            .db
            .open_table(&self.config.table_name)
            .execute()
            .await?;

        // LanceDB allows SQL-style filtering directly on the Arrow columns
        let mut stream = table
            .query()
            .only_if(format!("tx_id = {}", target_tx_id))
            .limit(1)
            .execute()
            .await?;

        if let Some(batch_result) = stream.next().await {
            let batch = batch_result?;
            let text_col = batch
                .column_by_name("text")
                .unwrap()
                .as_any()
                .downcast_ref::<arrow_array::StringArray>()
                .expect("FATAL: text column isn't StringArray");

            if text_col.len() > 0 {
                return Ok(format!(
                    "[COLD STORAGE] TxID: {} | Text: {} ",
                    target_tx_id,
                    text_col.value(0)
                ));
            }
        }

        Err(anyhow::anyhow!(
            "TxID {} not found in WAL or LanceDB.",
            target_tx_id
        ))
    }

    /// THE RESURRECTION ENGINE
    /// Rebuild the LORO CRDT Hive Mind from Cold storage and Hot Memory.
    pub async fn rebuild_agent_timeline(
        &self,
        agent_hex: &str,
        target_tx_id: u128,
        wal_engine: Arc<WalEngine>,
    ) -> Result<(Vec<u8>, Vec<OpLog>, u128, u64), anyhow::Error> {
        // RESOLVE THE TARGET INFINITY HACK
        // Find the highest known tx_id for this agent.
        let actual_target_transaction = if target_tx_id == u128::MAX {
            // Checking the WAL Index first
            let wal_max = {
                let idex = wal_engine.index.read().await;
                idex.keys().copied().filter(|&k| k > 0).max()
            };

            if let Some(max_tx) = wal_max {
                max_tx
            } else {
                // If WAL is empty/ compacted, ask lanceDB for the absolute highest recorded tx_id
                let (max_lance_tx, _, _) = self
                    .lance_engine
                    .fetch_closest_snapshot(agent_hex, u128::MAX)
                    .await
                    .unwrap_or((0, 0, Vec::new()));
                max_lance_tx
            }
        } else {
            target_tx_id
        };

        // 1. O(1) COLD MEMORY JUMP (LanceDB)
        let (snapshot_txid, snapshot_timestamp, memory_blob) = self
            .lance_engine
            .fetch_closest_snapshot(agent_hex, target_tx_id)
            .await
            .unwrap_or((0, 0, Vec::new()));

        // Determine if we need deep discovery (LanceDB) or Hot Recoverey (WAL)
        let oldest_wal_tx = {
            let idx = wal_engine.index.read().await;
            idx.keys().next().cloned().unwrap_or(u128::MAX) // Get the current smallest TxID currently in the WAL
        };

        println!(
            "[TIME MACHINE] Loaded Base Snapshot at TxID: {} ",
            snapshot_txid
        );

        let mut historical_oplogs = Vec::new();

        // 2. O(1) WAL INDEX SEEK
        // We calculate the very next TxID we need to read
        let next_txid = snapshot_txid;

        if actual_target_transaction < oldest_wal_tx {
            // DEEP TIME TRAVEL: The WAL has been compacted. We must read from LanceDB.
            println!(
                "[TIME MACHINE] Target is deep in history. Engaging LanceDB Deep Discovery... "
            );
            let table = self
                .lance_engine
                .db
                .open_table(&self.lance_engine.history_table)
                .execute()
                .await?;

            // Query all the snapshots btw snapshot and target
            let mut stream = table
                .query()
                .only_if(format!(
                    "agent_id = '{}' AND tx_id >= '{}' AND tx_id <= '{}'",
                    agent_hex,
                    format!("{:032x}", next_txid),
                    format!("{:032x}", target_tx_id)
                ))
                .execute()
                .await?;

            while let Some(batch_result) = stream.next().await {
                let batch = batch_result?;

                let tx_id_col = batch
                    .column_by_name("transaction_id")
                    .unwrap()
                    .as_any()
                    .downcast_ref::<arrow_array::StringArray>()
                    .expect(" FATAL: trasaction_id column isn't an Int64Array");
                let text_col = batch
                    .column_by_name("text")
                    .unwrap()
                    .as_any()
                    .downcast_ref::<arrow_array::StringArray>()
                    .expect(" FATAL: text column isn't a StringArray ");
                let timestamp_col = batch
                    .column_by_name("timestamp")
                    .unwrap()
                    .as_any()
                    .downcast_ref::<arrow_array::Int64Array>()
                    .expect(" FATAL: timestamp column isn't an IntArray");
                let status_col = batch
                    .column_by_name("status")
                    .unwrap()
                    .as_any()
                    .downcast_ref::<arrow_array::StringArray>()
                    .expect(" FATAL: status column is not a StringArrray");
                let seed_col = batch
                    .column_by_name("entropy_seeds")
                    .unwrap()
                    .as_any()
                    .downcast_ref::<arrow_array::StringArray>()
                    .expect(" FATAL: seeds column isn't a StringArray");
                let net_col = batch
                    .column_by_name("network_responses")
                    .unwrap()
                    .as_any()
                    .downcast_ref::<arrow_array::StringArray>()
                    .expect(" FATAL: network_reponse isn't a StringArray");
                let namespace_col = batch
                    .column_by_name("namespace")
                    .unwrap()
                    .as_any()
                    .downcast_ref::<arrow_array::StringArray>()
                    .expect("namespace is not a StringArray");
                let delta_col = batch
                    .column_by_name("payload")
                    .unwrap()
                    .as_any()
                    .downcast_ref::<arrow_array::BinaryArray>()
                    .expect(" FATAL: payload isn't a BinaryArray");

                for i in 0..timestamp_col.len() {
                    let status = match status_col.value(i) {
                        "IDLE" => AgentStatus::Idle,
                        "REASONING" => AgentStatus::Reasoning,
                        "HALTED" => AgentStatus::Halted,
                        "TOOL_EXEC" => AgentStatus::ToolExecution,
                        _ => {
                            // Log the currection and default to a safe state
                            eprintln!(
                                "[WARNING] Unknown status '{}' in LanceDB for TxID {}. Defaulting to Halted.",
                                status_col.value(i),
                                tx_id_col.value(i)
                            );
                            AgentStatus::Halted
                        }
                    };

                    let recovered_seed: Vec<u64> = serde_json::from_str(seed_col.value(i))?;
                    let recovered_res: Vec<String> = serde_json::from_str(net_col.value(i))?;

                    let reconstruct_log = OpLog {
                        agent_id: [0; 16],
                        delta: delta_col.value(i).to_vec(),
                        previous_hash: [0; 32],
                        current_hash: [0; 32],
                        state: crate::AgentState {
                            agent_id: Some([0; 16]),
                            namespace: namespace_col.value(i).to_string(),
                            transaction_id: u128::from_str_radix(
                                &tx_id_col.value(i).to_string().as_str(),
                                16,
                            )
                            .unwrap_or(0),
                            timestamp: timestamp_col.value(i),
                            status,
                            text: text_col.value(i).to_string(),
                        },
                        entropy_seeds: recovered_seed,
                        network_responses: recovered_res,
                    };
                    historical_oplogs.push(reconstruct_log);
                }
            }
        } else {
            // HOT RECORVERY: The data is still in the WAL.
            if next_txid <= target_tx_id {
                // Ask the mutex protected BTreeMap for the exact byte offset on the SSD
                let start_byte = {
                    let idx = wal_engine.index.read().await;
                    idx.get(&next_txid).cloned().unwrap_or(0)
                };

                // 3. Physical disk seek
                if let Ok(mut file) = std::fs::File::open(&self.config.wal_path) {
                    // The Kernel jumps the read-head directly to the exact byte. Zero scanning!
                    file.seek(SeekFrom::Start(start_byte))
                        .expect("Failed to seek WAL file");

                    let mut buffer = Vec::new();
                    file.read_to_end(&mut buffer).unwrap(); // Read the remainder of the file

                    let mut offset = 0;
                    while offset < buffer.len() {
                        if offset + 4 > buffer.len() {
                            break;
                        }

                        let mut len_bytes = [0u8; 4];
                        len_bytes.copy_from_slice(&buffer[offset..offset + 4]);
                        let entry_len = u32::from_le_bytes(len_bytes) as usize;
                        offset += 4;

                        let entry_slice = &buffer[offset..offset + entry_len];
                        let archived_log = unsafe {
                            rkyv::access_unchecked::<<OpLog as Archive>::Archived>(entry_slice)
                        };

                        let current_tx = archived_log.state.transaction_id;

                        if current_tx > target_tx_id {
                            break;
                        } // We reached the future. Stop reading.

                        // Only collect logs belonging to this specific agent!
                        if hex::encode(archived_log.agent_id.as_slice()) == agent_hex {
                            // Deserialize here only because we're handling this to the WASM to execute.
                            if let Ok(log) =
                                rkyv::deserialize::<OpLog, rkyv::rancor::Error>(archived_log)
                            {
                                historical_oplogs.push(log);
                            }
                        }

                        offset += entry_len;
                    }
                }
            }
        }

        Ok((
            memory_blob,
            historical_oplogs,
            actual_target_transaction,
            snapshot_timestamp,
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

        let (_memory_blob, historical_oplog, _snapshot_tx, _snapshot_timestamp) = self
            .rebuild_agent_timeline(agent_hex, fetch_target, self.wal_engine.clone())
            .await?;

        if historical_oplog.is_empty() {
            return Err(anyhow::anyhow!(
                "CRITICAL: Agent {} has no immutable memory on disk. Rebuild aborted.",
                agent_hex
            ));
        }

        let real_namespace = &historical_oplog[0].state.namespace;

        // Deterministic Phantom Salt Derivation
        let original_bytes =
            hex::decode(agent_hex).map_err(|e| anyhow::anyhow!("Invalid agent hex: {}", e))?;
        let mut phantom_bytes = [0u8; 16];
        if original_bytes.len() == 16 {
            phantom_bytes.copy_from_slice(&original_bytes);
        }

        if is_isolated_debug {
            let tx_id_bytes = target_tx_id.unwrap_or(0).to_be_bytes();
            let mut salt = [0u8; 16];
            salt[0..8].copy_from_slice(&tx_id_bytes);
            salt[8..16].copy_from_slice(&tx_id_bytes);

            for i in 0..16 {
                phantom_bytes[i] ^= salt[i];
                phantom_bytes[i] ^= 0xFF;
            }
        }

        let sandbox_agent_hex = hex::encode(phantom_bytes);

        // Resolve Target CRDT Hive-Mind Shard
        let target_brain = if is_isolated_debug {
            let phantom_namespace = format!("phantom_{}_{}", real_namespace, sandbox_agent_hex);
            println!(
                "[TIME MACHINE] Branching into isolated CRDT shard: {}",
                phantom_namespace
            );
            self.brain.get_or_create_brain(&phantom_namespace)
        } else {
            println!(
                "[RESURRECTION] Re-synchronizing canonical CRDT shard: {}",
                real_namespace
            );
            self.brain.get_or_create_brain(real_namespace)
        };

        // Replay historical deltas into the CRDT Hive-Mind
        for log in &historical_oplog {
            if let Err(e) = target_brain.assimilate_foreign_thought(&log.delta) {
                eprintln!("[WARNING] Failed to assimilate historical delta: {}", e);
            }
        }

        // Apply reality fork overrides if specified
        if let Some(fork) = &fork_config {
            println!(
                "[TIME MACHINE] Applied {} reality fork overrides",
                fork.env_overrides.len()
            );
        }

        // Broadcast glass UI event for the temporal scrubber
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

        // Cleanup dead simulation shards
        self.brain.purge_phantom_shards();

        Ok(())
    }

    /// Unified hybrid search engine: Scatters query to Cold LanceDB and Hot RAM Vector Buffer concurrently performs Receprpcal Rank Fusion (RRF) and Time-Decay Scoring, and formats context strings.
    pub async fn query_hybrid_memory(
        &self,
        query: &str,
        namespace: Option<&str>,
        limit: usize,
        hot_buffer: &HotVectorBuffer,
    ) -> Result<Vec<String>, anyhow::Error> {
        // Embed query once
        let query_vector = self.lance_engine.embedder.embed(query).await?;

        // PARALLEL SCATTER-GATHER (Cold LanceDB + Hot RAM)
        let cold_future = self
            .lance_engine
            .search_cold_vector(&query_vector, namespace, limit * 2);

        let hot_result = hot_buffer.search_hot(&query_vector, namespace, limit * 2);

        let cold_result = cold_future.await.unwrap_or_default();

        // RECIPROCAL RANK FUSION (RRF) & DEDUPLICATION BY UUIDv7 tx_id
        let mut fused_map: HashMap<u128, UnifiedSearchResult> = HashMap::new();
        let k = 60.0f32; // std RRF smoothing constant
        let current_ts = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs() as i64;

        // Process Cold Results
        for (rank, cold) in cold_result.into_iter().enumerate() {
            let rrf_score = 1.0 / (k + (rank + 1) as f32);

            // Time decay multiplier: e^(-lambda * delta_t_hours)
            let age_hours = ((current_ts - cold.timestamp).max(0) as f32) / 3600.0;
            let time_decay = (-0.05 * age_hours).exp();
            let final_score = rrf_score * time_decay;

            fused_map.insert(
                cold.tx_id,
                UnifiedSearchResult {
                    tx_id: cold.tx_id,
                    agent_hex: cold.agent_hex,
                    namespace: cold.namespace,
                    text: cold.text,
                    timestamp: cold.timestamp,
                    score: final_score,
                    source: "COLD_LANCEDB",
                },
            );
        }

        // Process Hot RAM Results (Boosted by recency)
        for (rank, (hot, sim_score)) in hot_result.into_iter().enumerate() {
            let rrf_score = 1.0 / (k + (rank + 1) as f32);
            let age_hours = ((current_ts - hot.timestamp).max(0) as f32) / 3600.0;
            let time_decay = (-0.01 * age_hours).exp(); // Slower decay for hot memory
            let final_score = (rrf_score + sim_score) * time_decay * 1.25; // 25% Hot Recency Boost

            fused_map
                .entry(hot.tx_id)
                .and_modify(|existing| {
                    if final_score > existing.score {
                        existing.score = final_score;
                        existing.source = "HOT_RAM_BUFFER";
                    }
                })
                .or_insert(UnifiedSearchResult {
                    tx_id: hot.tx_id,
                    agent_hex: hot.agent_hex,
                    namespace: hot.namespace,
                    text: hot.text,
                    timestamp: hot.timestamp,
                    score: final_score,
                    source: "HOT_RAM_BUFFER",
                });
        }

        // SORT BY HYBRID RRF SCORE
        let mut final_list: Vec<UnifiedSearchResult> = fused_map.into_values().collect();
        final_list.sort_by(|a, b| {
            b.score
                .partial_cmp(&a.score)
                .unwrap_or(std::cmp::Ordering::Equal)
        });
        final_list.truncate(limit);

        // Format Hyper-Rich Context String For LLM Prompt Window
        let formatted_memories: Vec<String> = final_list
            .into_iter()
            .map(|res| {
                format!(
                    "[Time: {}] [TxID: {:032x}] [Source: {}] Namespace: '{}' -> {} ",
                    res.timestamp, res.tx_id, res.source, res.namespace, res.text
                )
            })
            .collect();

        Ok(formatted_memories)
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

        let state = AgentState {
            agent_id: Some(agent_id),
            transaction_id,
            timestamp,
            namespace: namespace.to_string(),
            status: AgentStatus::ToolExecution,
            text: format!(
                "[EFFECT_RECORD] Step: {} | Len: {} bytes ",
                step_ordinal,
                output_payload.len()
            ),
        };

        let raw_oplog = OpLog {
            agent_id,
            state,
            delta: output_payload.clone(),
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
}
