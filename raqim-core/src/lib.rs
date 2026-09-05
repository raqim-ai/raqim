pub mod aegis;
pub mod api;
pub mod axon;
pub mod compactor;

pub mod config;
pub mod embedding;
pub mod health;
pub mod lancedb_store;
pub mod memory_router;
pub mod network;
pub mod nucleus;
pub mod registry;
pub mod state;
pub mod utils;

pub mod hot_memory;
pub mod witness;

use blake3::Hasher;
use rkyv::{Archive, Deserialize, Serialize};
use serde::{Deserialize as SerdeDeserialize, Serialize as SerdeSerialize};
use std::format;
use std::sync::atomic::AtomicBool;
use std::sync::{Arc, RwLock};
use std::time::{SystemTime, UNIX_EPOCH};
use tokio::sync::broadcast::Sender;

use crate::aegis::QuarantineRecord;
use crate::axon::MarkleBatch;
use crate::state::SwarmStateRegistry;
use crate::{axon::AxonGateKeeper, network::GlobalNetworkBridge, nucleus::WalEngine};

// The fundamental unit of our Flight Recorder.
#[derive(
    Archive, Deserialize, Serialize, Debug, PartialEq, Clone, SerdeDeserialize, SerdeSerialize,
)]
pub struct AgentState {
    pub agent_id: Option<[u8; 16]>,
    pub transaction_id: u128,

    pub timestamp: i64,
    pub status: AgentStatus,

    pub text: String,
    pub namespace: String,
}

// The current execution state of the agent in the swarm.
#[derive(
    Archive, Deserialize, Serialize, Debug, PartialEq, Clone, SerdeDeserialize, SerdeSerialize,
)]
pub enum AgentStatus {
    Idle,
    Reasoning,     // Waiting on LLM token generation
    ToolExecution, // Executing an external API or tool
    Halted,        // Interdicted by the Aegis security layer
}

// Every thought and action is an Op.
#[derive(
    Archive, Deserialize, Serialize, Debug, PartialEq, Clone, SerdeDeserialize, SerdeSerialize,
)]
pub struct OpLog {
    pub agent_id: [u8; 16],
    pub state: AgentState,

    pub delta: Vec<u8>,

    pub previous_hash: [u8; 32],
    pub current_hash: [u8; 32],

    // The deterministic flight recorder
    pub entropy_seeds: Vec<u64>,
    pub network_responses: Vec<String>,
}

#[derive(Archive, Serialize, Deserialize, Debug, Clone)]
#[rkyv(compare(PartialEq))]
pub struct A2AEnvelope {
    pub sender_id: [u8; 16],
    pub sender_public_key: [u8; 32],
    pub target_capability: String,
    pub payload: Vec<u8>,
    pub signature: [u8; 64],
    pub sender_capability_cert: Vec<u8>,
    pub timestamp: i64,
}

#[derive(Archive, Deserialize, Serialize, Debug, Clone)]
pub struct IngressEnvelope {
    pub intent_path: String,
    pub public_key: [u8; 32],
    pub signature: [u8; 64],
    pub state_bytes: Vec<u8>,
    pub capability_cert: Vec<u8>,
}

/// The Cryptographic Boundary Representation of a Non-Deterministoc Side-Effect
#[derive(Debug, Clone, Archive, Serialize, Deserialize, serde::Serialize, serde::Deserialize)]
pub struct EffectRecord {
    pub agent_id: [u8; 16],
    pub step_ordinal: u64,

    /// 32-byte Blake3 hash of the input signature (Prompt text + model name + parameters)
    pub call_signature_hash: [u8; 32],

    pub output_payload: Vec<u8>,

    /// 128-bit transaction ID binding this effect to the Merkle DAG
    pub transaction_id: u128,

    pub timestamp: i64,
}

/// The Unique Lookup Key computed fpr in-memory RAM Effect matching
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct EffectKey(pub [u8; 32]);

impl EffectKey {
    /// Computes a domain-separated BLAKE3 key over (agent_id + step_ordinal + call_signature_hash)
    pub fn derive(agent_id: &[u8; 16], step_ordinal: u64, call_hash: &[u8; 32]) -> Self {
        let mut hasher = Hasher::new_derive_key("raqim.effect.v1.key");
        hasher.update(agent_id);
        hasher.update(&step_ordinal.to_le_bytes());
        hasher.update(call_hash);

        let mut key_bytes = [0u8; 32];
        hasher.finalize_xof().fill(&mut key_bytes);
        Self(key_bytes)
    }
}

#[inline(always)]
pub fn generate_uuidv7_txid() -> u128 {
    uuid::Uuid::now_v7().as_u128()
}

pub async fn execute_raqim_cascade(
    archive_state: &rkyv::Archived<AgentState>, // True Zero Copy
    axon: Arc<AxonGateKeeper>,
    wal: Arc<WalEngine>,
    shard_brain: Arc<SwarmStateRegistry>,
    global_net: Arc<GlobalNetworkBridge>,
    tx: Sender<SystemEvent>,
    seeds: Vec<u64>,
    responses: Vec<String>,
) -> Result<u128, anyhow::Error> {
    // Security: Validate or generate agent_id
    let empty_id = [0u8; 16];

    // Safely extract from ArrchiveOption using .as_ref()
    let final_agent_id = match archive_state.agent_id.as_ref() {
        Some(id) if id.as_slice() != empty_id => id.as_slice().try_into().unwrap(),
        _ => {
            eprintln!("[SECURITY FATAL] Unsigned/Anonymous payload hit the cascade. Dropped.");
            return Err(anyhow::anyhow!("Agent ID is required to merge thought"));
        }
    };

    let agent_hex = hex::encode(final_agent_id);

    // Generate globally uniique, time-ordered 128-bit UUIDv7
    let tx_id = generate_uuidv7_txid();

    let enriched_state = AgentState {
        agent_id: Some(final_agent_id),
        namespace: archive_state.namespace.to_string(),
        transaction_id: tx_id,

        timestamp: SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_secs() as i64,
        status: match archive_state.status {
            rkyv::Archived::<AgentStatus>::Idle => AgentStatus::Idle,
            rkyv::Archived::<AgentStatus>::Halted => AgentStatus::Halted,
            rkyv::Archived::<AgentStatus>::ToolExecution => AgentStatus::ToolExecution,
            rkyv::Archived::<AgentStatus>::Reasoning => AgentStatus::Reasoning,
        },
        text: archive_state.text.as_str().to_string(), // Extract text from pointer
    };

    let delta = shard_brain
        .get_or_create_brain(&enriched_state.namespace.clone())
        .append_agent_thought(&agent_hex, &enriched_state)
        .map_err(|e| anyhow::anyhow!("CRDT allocation failed: {}", e))?;

    // telemetry.record_crdt_merge();

    // Contruct the raw log
    let raw_log = OpLog {
        agent_id: final_agent_id,
        state: enriched_state.clone(),
        delta,
        previous_hash: [0; 32],
        current_hash: [0; 32],

        entropy_seeds: seeds,
        network_responses: responses,
    };

    // 3. Cryptographically Seal (Markle DAG)
    let (sealed_log, optional_markle_batch) = axon.seal_thought(raw_log);

    if let Some(batch) = optional_markle_batch {
        let _ = tx.send(SystemEvent::MarkleBatchCrystallized { batch });
    }

    // 4. Fire to wal (Durability)
    wal.append(sealed_log.clone())
        .await
        .map_err(|e| anyhow::anyhow!("Durability Breach / WAL Saturated: {}", e))?;

    global_net.broadcast_to_world(&sealed_log).await;

    // Convert u128 to hex string for human-readable sse event
    let tx_id_hex = format!("{:032x}", tx_id);

    let _ = tx.send(SystemEvent::ThoughtCommitted {
        agent_id: agent_hex.clone(),
        tx_id: tx_id_hex,
        namespace: enriched_state.clone().namespace,
        text: enriched_state.clone().text,
    });

    Ok(tx_id)
}

#[derive(Clone, Debug, Archive, Serialize, Deserialize, SerdeSerialize, SerdeDeserialize)]
pub enum SystemEvent {
    ThoughtCommitted {
        agent_id: String,
        tx_id: String,
        namespace: String,
        text: String,
    },
    SecurityBreach {
        agent_id: String,
        reason: String,
        culprit_text: String,
    },

    RealityForked {
        agent_id: String,
        original_namespace: String,
        phantom_namespace: String,
        step_ordinal: u64,
        tx_id: String,
        timestamp: i64,
    },

    AegisInterdiction {
        agent_id: String,
        attempted_path: String,
        rule_broken: String,
        payload: String,
    },

    CompactionTriggered {
        archived_count: usize,
        max_compacted_tx: u128,
    },
    PluginLoaded {
        plugin_name: String,
    },

    SystemBoot {
        message: String,
    },
    LicenseUpdated {
        new_jwt: String,
    },

    GlobalQuarantineSync {
        record: QuarantineRecord,
    },

    MarkleBatchCrystallized {
        batch: MarkleBatch,
    },
}

#[derive(Debug, Clone)]
pub struct RuntimeSecurityFlags {
    pub allow_global_a2a: Arc<AtomicBool>,
    pub allow_global_crdt: Arc<AtomicBool>,
    pub allow_global_aegis: Arc<AtomicBool>,
    pub allow_time_travel: Arc<AtomicBool>,
    pub tenant_id: Arc<RwLock<String>>,
}

impl RuntimeSecurityFlags {
    pub fn new() -> Self {
        RuntimeSecurityFlags {
            allow_global_a2a: Arc::new(AtomicBool::new(true)),
            allow_global_aegis: Arc::new(AtomicBool::new(true)),
            allow_global_crdt: Arc::new(AtomicBool::new(true)),
            allow_time_travel: Arc::new(AtomicBool::new(true)),
            tenant_id: Arc::new(RwLock::new("local_standalone".to_string())),
        }
    }
}

impl Default for RuntimeSecurityFlags {
    fn default() -> Self {
        Self::new()
    }
}
