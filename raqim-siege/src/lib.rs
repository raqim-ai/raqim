use rkyv::{Archive, Deserialize, Serialize};
use serde::{Deserialize as SerdeDeserialize, Serialize as SerdeSerialize};

#[derive(Archive, Deserialize, Serialize, Debug, Clone)]
pub struct IngressEnvelope {
    pub intent_path: String,      // "raqim_finance/ledger" ( Checked by Aegis )
    pub public_key: [u8; 32],     // The Ed25519 public key of the sender
    pub signature: [u8; 64],      // The mathematical signauture proving authenticity
    pub state_bytes: Vec<u8>,     // The actual thought
    pub capability_cert: Vec<u8>, // The master token signed by the Master Key
}

#[derive(Archive, Deserialize, Serialize, Debug, PartialEq, Clone)]
pub struct AgentState {
    pub agent_id: Option<[u8; 16]>,
    pub transaction_id: u128,

    pub timestamp: i64,
    pub status: AgentStatus,

    pub text: String,
    pub namespace: String,
}

#[derive(Archive, Deserialize, Serialize, Debug, PartialEq, Clone)]
pub enum AgentStatus {
    Idle,
    Reasoning,     // Waiting on LLM token generation
    ToolExecution, // Executing an external API or tool
    Halted,        // Interdicted by the Aegis security layer
}

#[derive(SerdeDeserialize, SerdeSerialize, Clone)]
pub struct CapabilityCertificate {
    pub agent_hex: String,
    pub group_name: String,
    pub expiration_timestamp: u64,
    pub master_signature: Vec<u8>,
}
