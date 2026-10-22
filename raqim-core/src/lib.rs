pub mod api;
pub mod checkpoint;
pub mod cold_store;
pub mod compactor;
pub mod config;
pub mod effect_store;
pub mod merkle;
pub mod otel;
pub mod policy;
pub mod registry;
pub mod utils;
pub mod wal;
pub mod witness;

#[cfg(feature = "vector_embedding")]
pub mod embedding;
#[cfg(feature = "vector_embedding")]
pub mod hot_memory;
#[cfg(feature = "vector_embedding")]
pub mod lancedb_store;

use blake3::Hasher;
use rkyv::{Archive, Deserialize, Serialize};
use serde::{Deserialize as SerdeDeserialize, Serialize as SerdeSerialize};
use std::sync::Arc;
use std::time::{SystemTime, UNIX_EPOCH};
use std::{eprintln, format};
use tokio::sync::broadcast::Sender;

use crate::merkle::MarkleBatch;
use crate::policy::QuarantineRecord;
use crate::{merkle::MerkleEngine, wal::WalEngine};

/// Lifecycle stage of an agent action at the control/gateway boundary
#[derive(
    Archive,
    Deserialize,
    Serialize,
    Debug,
    PartialEq,
    Eq,
    Clone,
    Copy,
    SerdeDeserialize,
    SerdeSerialize,
)]
pub enum ExecutionStage {
    /// Pre-execution intent logged to WAL *before* upstream API or tool runs.
    Intent,
    /// Post-execution outcome sealed with output payload and Merkle leaf hash.
    Outcome,
    /// Action rejected pre-execution by PolicyEngine
    Denied,
    /// Lifecycle / system events (e.g., agent boot handshake)
    System,
}

impl ExecutionStage {
    pub fn as_str(&self) -> &'static str {
        match self {
            ExecutionStage::Intent => "INTENT",
            ExecutionStage::Outcome => "OUTCOME",
            ExecutionStage::Denied => "DENIED",
            ExecutionStage::System => "SYSTEMS",
        }
    }
}

/// The Unified Crptographic Flight Record. Supports NDPA/GDPR Crypto-Shredding & Erasure without breaking Merkle proofs.
#[derive(
    Archive, Deserialize, Serialize, Debug, PartialEq, Eq, Clone, SerdeDeserialize, SerdeSerialize,
)]
pub struct ExecutionRecord {
    pub tx_id: u128,
    pub agent_id: [u8; 16],
    pub step_ordinal: u64,
    pub timestamp: i64, // ms
    pub namespace: String,
    /// Current stage in the execution state machine
    pub stage: ExecutionStage,
    pub call_signature_hash: [u8; 32],
    pub payload_hash: [u8; 32],
    pub payload_bytes: Vec<u8>,
    pub previous_root: [u8; 32],
    pub leaf_hash: [u8; 32],
}

impl ExecutionRecord {
    /// computes the domain-separated 32-byte blake3 digest of a payload slice.
    pub fn compute_payload_hash(payload_bytes: &[u8]) -> [u8; 32] {
        let mut hasher = Hasher::new_derive_key("raqim.payload.v1");
        hasher.update(payload_bytes);
        hasher.finalize().into()
    }

    pub fn new(
        agent_id: [u8; 16],
        step_ordinal: u64,
        payload_bytes: Vec<u8>,
        stage: ExecutionStage,
        namespace: String,
        call_signature_hash: [u8; 32],
    ) -> Self {
        let tx_id = generate_uuidv7_txid();
        let timestamp = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_millis() as i64;
        let payload_hash = Self::compute_payload_hash(&payload_bytes);

        Self {
            tx_id,
            agent_id,
            timestamp,
            namespace,
            step_ordinal,
            stage,
            call_signature_hash,
            payload_hash,
            payload_bytes,
            previous_root: [0u8; 32],
            leaf_hash: [0u8; 32],
        }
    }

    /// Helper to display payload as UTF-8 text preview
    pub fn payload_text_preview(&self) -> String {
        if self.payload_bytes.is_empty() && self.payload_hash != Self::compute_payload_hash(&[]) {
            "[REDACTED_NDPA]".to_string()
        } else {
            String::from_utf8(self.payload_bytes.clone())
                .unwrap_or_else(|_| format!("[BINARY_PAYLOAD: {} bytes]", self.payload_bytes.len()))
        }
    }
}

#[derive(Debug, PartialEq, Clone, SerdeDeserialize, SerdeSerialize)]
pub struct RecentRecord {
    pub tx_id: String,
    pub tx_id_hex: String,
    pub agent_hex: String,
    pub intent_path: String,
    pub text: String,
    pub status: String,
    pub timestamp: i64,
}

/// Signed wire evellope for high-throughput binary TCP / HTTP ingress
#[derive(Archive, Deserialize, Serialize, Debug, Clone)]
pub struct IngressEnvelope {
    pub intent_path: String,
    pub public_key: [u8; 32],
    pub signature: [u8; 64],
    pub record_bytes: Vec<u8>,
    pub capability_cert: Vec<u8>,
}

/// The Unique Lookup Key computed for in-memory RAM Effect matching & In-Doubt tracking.
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

/// Zero-copy TCP ingress cascade: validates and seals an incoming `Archived<Execution>`
pub async fn execute_raqim_cascade(
    archived_rec: &rkyv::Archived<ExecutionRecord>,
    merkle: Arc<MerkleEngine>,
    wal: Arc<WalEngine>,
    tx: Sender<SystemEvent>,
) -> Result<u128, anyhow::Error> {
    // Security: Validate or generate agent_id
    let empty_id = [0u8; 16];

    let agent_id: [u8; 16] = archived_rec
        .agent_id
        .as_slice()
        .try_into()
        .unwrap_or(empty_id);
    if agent_id == empty_id {
        eprintln!("[SECURITY FATAL] Anonymous Execution hit the cascade. Dropped.");
        return Err(anyhow::anyhow!("Non-zero Agent ID is required"));
    }

    let agent_hex = hex::encode(agent_id);
    let namespace = archived_rec.namespace.as_str().to_string();
    let step_ordinal = archived_rec.step_ordinal.to_native();
    let call_signature_hash: [u8; 32] = archived_rec
        .call_signature_hash
        .as_slice()
        .try_into()
        .unwrap_or([0u8; 32]);
    let payload_bytes = archived_rec.payload_bytes.as_slice().to_vec();

    let stage = match archived_rec.stage {
        rkyv::Archived::<ExecutionStage>::Intent => ExecutionStage::Intent,
        rkyv::Archived::<ExecutionStage>::Outcome => ExecutionStage::Outcome,
        rkyv::Archived::<ExecutionStage>::Denied => ExecutionStage::Denied,
        rkyv::Archived::<ExecutionStage>::System => ExecutionStage::System,
    };

    let unsealed_record = ExecutionRecord::new(
        agent_id,
        step_ordinal,
        payload_bytes,
        stage,
        namespace.clone(),
        call_signature_hash,
    );
    let tx_id = unsealed_record.tx_id;
    let text_preview = unsealed_record.payload_text_preview();

    // 1. Cryptographically seal into BLAKE3 Merkle DAG
    let (sealed_record, optional_batch) = merkle.seal_record(unsealed_record);

    if let Some(batch) = optional_batch {
        let _ = tx.send(SystemEvent::MarkleBatchCrystallized { batch });
    }

    wal.append(sealed_record)
        .await
        .map_err(|e| anyhow::anyhow!("Durability Breach / WAL Saturated: {}", e))?;

    let _ = tx.send(SystemEvent::ThoughtCommitted {
        agent_id: agent_hex,
        tx_id: format!("{:032x}", tx_id),
        namespace,
        text: text_preview,
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
