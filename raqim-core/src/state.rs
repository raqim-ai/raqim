// =============================================
// RAQIM LOCK-FREE NAMESPACE & TIMELINE TRACKER
// =============================================
// Tracks active namespaces, agent timeline and op counts using lock-free AtomicUsize and Dashmap Shards.

use crate::AgentState;
use dashmap::{DashMap, DashSet};
use std::collections::HashMap;
use std::sync::atomic::Ordering::Relaxed;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;

pub struct SwarmState {
    pub namespace: String,
    pub op_count: AtomicUsize,
    pub active_agents: DashSet<String>,
}

impl SwarmState {
    pub fn new(ns: &str) -> Self {
        Self {
            namespace: ns.to_string(),
            op_count: AtomicUsize::new(0),
            active_agents: DashSet::new(),
        }
    }

    pub fn append_agent_thought(&self, agent_id_hex: &str) -> Result<Vec<u8>, anyhow::Error> {
        self.active_agents.insert(agent_id_hex.to_string());
        self.op_count.fetch_add(1, Relaxed);

        Ok(Vec::new())
    }

    pub fn assimillate_foreign_thought(&self, _delta: &[u8]) -> Result<(), anyhow::Error> {
        self.op_count.fetch_add(1, Ordering::Relaxed);
        Ok(())
    }

    pub fn len_ops(&self) -> usize {
        self.op_count.load(Ordering::Relaxed)
    }

    pub fn assimilate_foreign_thought(&self, _delta: &[u8]) -> Result<(), anyhow::Error> {
        self.op_count.fetch_add(1, Ordering::Relaxed);
        Ok(())
    }

    pub fn active_timeline_count(&self) -> usize {
        self.active_agents.len()
    }
}

pub struct SwarmStateRegistry {
    pub shards: DashMap<String, Arc<SwarmState>>,
}

impl SwarmStateRegistry {
    pub fn new() -> Self {
        Self {
            shards: DashMap::new(),
        }
    }

    pub fn get_or_create_brain(&self, namespace: &str) -> Arc<SwarmState> {
        if let Some(state) = self.shards.get(namespace) {
            return state.value().clone();
        }
        let new_brain = Arc::new(SwarmState::new(namespace));
        self.shards
            .entry(namespace.to_string())
            .or_insert(new_brain)
            .value()
            .clone()
    }

    pub fn evict_brain(&self, namespace: &str) -> bool {
        self.shards.remove(namespace).is_some()
    }

    pub fn purge_phantom_shards(&self) -> usize {
        let mut purge_count = 0;
        self.shards.retain(|key, brain| {
            if key.starts_with("phantom_") && Arc::strong_count(brain) == 1 {
                purge_count += 1;
                return false;
            }
            true
        });
        purge_count
    }

    pub fn export_all_snapshots(&self) -> HashMap<String, Vec<u8>> {
        let mut map = HashMap::new();
        for entry in self.shards.iter() {
            let count_bytes = (entry.value().len_ops() as u64).to_le_bytes().to_vec();
            map.insert(entry.key().clone(), count_bytes);
        }
        map
    }

    pub fn import_snapshots(&self, snapshots: HashMap<String, Vec<u8>>) {
        for (ns, bytes) in snapshots {
            let brain = self.get_or_create_brain(&ns);
            if bytes.len() == 8 {
                let count = u64::from_le_bytes(bytes.try_into().unwrap()) as usize;
                brain.op_count.store(count, Ordering::Relaxed);
            }
        }
    }
}
