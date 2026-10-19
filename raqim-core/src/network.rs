// ==============================================================================
// RAQIM LOCAL CONTROL BRIDGE (Single-Node Zero-Latency Implementation)
// ==============================================================================
// Replaces the heavy Zenoh multicast P2P runtime in v0.2.0 single-node mode.
// Startup time: 0.00ms. Memory overhead: 0 bytes.
// ==============================================================================

use crate::merkle::MerkleEngine;
use crate::policy::{PolicyEngine, QuarantineRecord};
use crate::state::SwarmStateRegistry;
use crate::{A2AEnvelope, OpLog, SystemEvent};
use serde::{Deserialize, Serialize};
use std::sync::Arc;
use tokio::sync::broadcast::Sender;

pub struct GlobalNetworkBridge {
    pub os_node_id: String,
}

#[derive(Clone, Debug, Serialize, Deserialize, rkyv::Archive, rkyv::Serialize)]
pub struct QuarantineWireEnvelope {
    pub origin_node_id: String,
    pub record: QuarantineRecord,
}

impl GlobalNetworkBridge {
    pub async fn new(
        _tenant_id: &str,
        _swarm_name: &str,
        _aegis: Arc<PolicyEngine>,
        os_node_id: String,
    ) -> Self {
        Self { os_node_id }
    }

    pub async fn broadcast_to_world(&self, _log: &OpLog) {}

    pub async fn execute_a2a_rpc(
        &self,
        envelope: A2AEnvelope,
        _aegis: Arc<PolicyEngine>,
    ) -> Result<(Vec<u8>, String), anyhow::Error> {
        Err(anyhow::anyhow!(
            "A2A Mesh disabled in single-node gate mode (target: {})",
            envelope.target_capability
        ))
    }

    pub async fn broadcast_quarantine_sync(&self, _record: QuarantineRecord) {}

    pub async fn listen_for_global_quarantine(&self, _aegis: Arc<PolicyEngine>) {}

    pub async fn listen_for_foreign_thoughts(
        &self,
        _brain_registry: Arc<SwarmStateRegistry>,
        _merkle: Arc<MerkleEngine>,
        _tx: Sender<SystemEvent>,
    ) {
    }

    pub async fn register_agent_capability<F>(&self, _capability_path: &str, _response_handler: F)
    where
        F: FnMut(&[u8]) -> Vec<u8> + Send + 'static,
    {
    }

    pub async fn dispatch_control_override(&self, _target_agent_hex: &str, _system_prompt: &str) {}

    pub async fn shutdown(&self) {}
}
