use std::time::{SystemTime, UNIX_EPOCH};

use dashmap::DashMap;
use serde::{Deserialize, Serialize};

#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct AgentProcess {
    pub agent_hex: String,
    pub alias: String,
    pub namespace: String,
    pub last_seen_ts: u64,
    pub status: String,
}

pub struct SwarmRegistry {
    // The Live Process Table
    pub active_agents: DashMap<String, AgentProcess>,
}

impl SwarmRegistry {
    pub fn new() -> Self {
        Self {
            active_agents: DashMap::new(),
        }
    }

    /// O(1) update function called during TCP ingress
    pub fn touch_agent(&self, agent_hex: &str, namespace: &str, status: &str, alias: &str) {
        let now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_secs();

        self.active_agents.insert(
            agent_hex.to_string(),
            AgentProcess {
                agent_hex: agent_hex.to_string(),
                alias: alias.to_string(),
                namespace: namespace.to_string(),
                last_seen_ts: now,
                status: status.to_string(),
            },
        );
    }

    /// Flags an agent as quarantined instantly acros
    /// s the UI
    pub fn quarantine_agent(&self, agent_hex: &str) {
        if let Some(mut process) = self.active_agents.get_mut(agent_hex) {
            process.status = "Quarantined".to_string();
        }
    }

    /// Exports all live agent processes for checkpointing
    pub fn export_agents(&self) -> Vec<AgentProcess> {
        self.active_agents.iter().map(|e| e.value().clone()).collect()
    }

    /// Hydrates agent processes from a checkpoint
    pub fn hydrate_agents(&self, agents: Vec<AgentProcess>) {
        for a in agents {
            self.active_agents.insert(a.agent_hex.clone(), a);
        }
    }
}
