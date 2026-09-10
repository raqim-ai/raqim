use crate::{AgentState, AgentStatus};
use dashmap::DashMap;
use loro::{ImportStatus, LoroDoc, LoroList, LoroMap};
use parking_lot::RwLock;
use std::{borrow::Cow, println, sync::Arc};

// The isolated CRDT Memory Shard for a Single Swarm namespace
pub struct SwarmState {
    // Thread safe parking_lot RWLock guaranntees zero lock poisoning and .01ms lock times.
    pub doc: RwLock<LoroDoc>,
    pub root_timeline_map: LoroMap,
}

impl SwarmState {
    /// Initialize the CRDT brain for a specific swarm domain.
    pub fn new(swarm_namespace: &str) -> Self {
        let doc = LoroDoc::new();

        // LoroDocs acts likes a filesystem. We create a root dir (Map) for our swarm.
        // Every swarm namespace recieves an isolated, independent root memory dictionary.
        let root_timeline_map = doc.get_map(swarm_namespace);

        // --- THE CRDT EVENT LISTENER ---
        // We attach a deep listener to the Loro Doc. Whenever the math resolves a conflict, this closure fires syncronously
        let _ = doc.subscribe_root(Arc::new(move |event: loro::event::DiffEvent| {
            // event.events contains the precise diffs (what was added, deleted, updated)
            for diff in &event.events {
                println!(
                    "[CRDT BRAIN] Syncronized event state delta at path: {:?}",
                    diff.path
                );
            }
        }));

        Self {
            doc: RwLock::new(doc),
            root_timeline_map,
        }
    }

    /// Appends a new thought securely to an agent's timeline array
    pub fn append_agent_thought(
        &self,
        agent_id_hex: &str,
        state: &AgentState,
    ) -> Result<Vec<u8>, anyhow::Error> {
        // Acquire an exclusive write lock ONLY for this specific swarm document
        let doc_lock = self.doc.write();

        let previous_vv = doc_lock.oplog_vv();

        // Checks if a timeline array already exists for this agent_id token
        let agent_timeline = match self.root_timeline_map.get(agent_id_hex) {
            Some(loro::ValueOrContainer::Container(container)) => container.into_list().unwrap(),

            _ => {
                // If this is the agent's first block, initialize a clean timeline list container
                self.root_timeline_map
                    .insert_container(agent_id_hex, LoroList::new())
                    .map_err(|_| {
                        anyhow::anyhow!(
                            "CRDT Allocation Error: Failed to generate agent timeine leaf "
                        )
                    })?
            }
        };

        // Construst a fresh, isolated state record snapshot map
        let record_entry = LoroMap::new();

        // Populate the historical frame leaf fields safely
        let hex_tx = format!("{:032x}", state.transaction_id);
        let _ = record_entry.insert("tx_id", hex_tx.as_str());
        let _ = record_entry.insert("ts", state.timestamp);
        let _ = record_entry.insert("payload", state.text.clone());

        let status_str = match state.status {
            AgentStatus::Idle => "IDLE",
            AgentStatus::Reasoning => "REASONING",
            AgentStatus::Halted => "HALTED",
            AgentStatus::ToolExecution => "TOOL_EXEC",
        };
        let _ = record_entry.insert("status", status_str);

        // Push the completed leaf node to the end of the agent's historical timeline array
        agent_timeline.insert_container(agent_timeline.len(), record_entry)?;

        // Force CRDT commit
        doc_lock.commit();

        // Export only the fresh structure mutatitons generated this specific Op
        let delta_bytes = doc_lock.export(loro::ExportMode::Updates {
            from: Cow::Borrowed(&previous_vv),
        })?;

        Ok(delta_bytes)
    }

    /// Merges another agent's thought (action) into this agent's brain. Conflict resolution happens here automatically.
    pub fn assimilate_foreign_thought(
        &self,
        delta: &[u8],
    ) -> Result<ImportStatus, loro::LoroError> {
        // parses the binary data and merges it into out local graph.
        self.doc.write().import(delta)
    }
}

// The Global Enterprise Registry.
pub struct SwarmStateRegistry {
    /// Sharded concurrent map mapping namespacestrigs to atomic brain pointers
    pub shards: DashMap<String, Arc<SwarmState>>,
}

impl SwarmStateRegistry {
    pub fn new() -> Self {
        Self {
            shards: DashMap::new(),
        }
    }

    /// TWO-PASS speculative allocation: Guarantees Zero lock starvation
    pub fn get_or_create_brain(&self, namespace: &str) -> Arc<SwarmState> {
        // Pass 1: Fast read path (Zero write-lock contention)
        if let Some(state) = self.shards.get(namespace) {
            return state.value().clone();
        }

        // Heavy work outside the lock: Dashmap is not locked during this allocation!
        let new_brain = Arc::new(SwarmState::new(namespace));

        // Pass 2: Atomic Entry (Fast Atomic Swap)
        self.shards
            .entry(namespace.to_string())
            .or_insert(new_brain)
            .value()
            .clone()
    }

    /// Explicit Eviction: Evicts a specific namespace shard from RAM hen terminated
    pub fn evict_brain(&self, namespace: &str) -> bool {
        self.shards.remove(namespace).is_some()
    }

    /// Atomic Purge: Scans and purges all dead/completed phantom simulation shard
    pub fn purge_phantom_shards(&self) -> usize {
        let mut purge_count = 0;

        // Retain only production shards & Active phantom shards still held by rurnning tasks
        self.shards.retain(|key, brain| {
            if key.starts_with("phantom_") {
                if Arc::strong_count(brain) == 1 {
                    purge_count += 1;
                    return false;
                }
            }
            true
        });

        if purge_count > 0 {
            println!(
                "[SWARM REGISTRY] Evicted {} dead phantom simulation shards from RAM. ",
                purge_count
            );
        }

        purge_count
    }
}
