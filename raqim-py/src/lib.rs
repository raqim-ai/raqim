use ed25519_dalek::{Signer, SigningKey};
use pyo3::prelude::*;
use pyo3::types::PyBytes;
use rkyv::{Archive, Deserialize, Serialize};
use serde::{Deserialize as SerdeDeserialize, Serialize as SerdeSerialize};
use std::{
    println,
    time::{SystemTime, UNIX_EPOCH},
};

#[derive(SerdeSerialize, SerdeDeserialize, Debug, Clone)]
pub struct CapabilityCertificate {
    pub agent_hex: String,
    pub group_name: String,
    pub expiration_timestamp: u64,
    pub master_signature: Vec<u8>, // Signed by Swarm Master Key
}

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

#[derive(Archive, Deserialize, Serialize, Debug, Clone)]
pub struct IngressEnvelope {
    pub intent_path: String,
    pub public_key: [u8; 32],
    pub signature: [u8; 64],
    pub state_bytes: Vec<u8>,
    pub capability_cert: Vec<u8>,
}

/// The Python Class wrapping the Rust Cryptography
#[pyclass]
struct RaqimCryptoCore {
    signing_key: SigningKey,

    #[pyo3(get)]
    pub_key_bytes: [u8; 32],

    #[pyo3(get)]
    capability_cert_bytes: Vec<u8>,
}

#[pymethods]
impl RaqimCryptoCore {
    #[new]
    fn new(pem_path: &str, cert_path: Option<&str>) -> PyResult<Self> {
        // PyO3 automatically translate std::io::Error from fs::read into python IOError!
        let key_bytes = std::fs::read(pem_path)?;
        let key_array: [u8; 32] = key_bytes
            .as_slice()
            .try_into()
            .map_err(|_| pyo3::exceptions::PyValueError::new_err("Private Key must be 32 bytes"))?;
        let signing_key = SigningKey::from_bytes(&key_array);
        let pub_key_bytes = signing_key.verifying_key().to_bytes();

        let mut hasher = blake3::Hasher::new_derive_key("raqim.agent.v1.identity");
        hasher.update(&pub_key_bytes);
        let mut agent_id_bytes = [0u8; 16];
        hasher.finalize_xof().fill(&mut agent_id_bytes);
        let agent_hex = hex::encode(agent_id_bytes);

        // 2. Load Capability Certificate
        let capability_cert = if let Some(path) = cert_path {
            if std::path::Path::new(path).exists() {
                std::fs::read(path).unwrap_or_default()
            } else {
                println!(" cert path: {} does not exist in file dir", path);
                Self::mint_capability_certificate(&agent_hex, &signing_key)
            }
        } else {
            println!("cert_path is set to None");
            Self::mint_capability_certificate(&agent_hex, &signing_key)
        };

        Ok(Self {
            signing_key,
            pub_key_bytes,
            capability_cert_bytes: capability_cert,
        })
    }

    /// Signs any raw byte array and returns the 64-byte signature
    fn sign_payload<'py>(&self, py: Python<'py>, payload: &[u8]) -> PyResult<Bound<'py, PyBytes>> {
        let signature = self.signing_key.sign(payload).to_bytes();
        Ok(PyBytes::new(py, &signature))
    }

    /// Exposes the active capability certificate to the python runtime
    #[getter]
    fn capability_cert_bytes<'py>(&self, py: Python<'py>) -> Bound<'py, PyBytes> {
        PyBytes::new(py, &self.capability_cert_bytes)
    }

    /// Converts Python strings directly into zero-copy TCP payload
    fn generate_tcp_payload<'py>(
        &self,
        py: Python<'py>,
        agent_hex: &str,
        intent_path: &str,
        text: &str,
    ) -> PyResult<Bound<'py, PyBytes>> {
        let agent_id_bytes = hex::decode(agent_hex)
            .map_err(|e| pyo3::exceptions::PyValueError::new_err(e.to_string()))?;

        let agent_id_array: [u8; 16] = agent_id_bytes.as_slice().try_into().map_err(|_| {
            pyo3::exceptions::PyValueError::new_err("Agent hex must be exactly 16-bytes")
        })?;

        let state = AgentState {
            agent_id: Some(agent_id_array),
            namespace: intent_path.to_string(),
            transaction_id: uuid::Uuid::now_v7().as_u128(),
            timestamp: SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap()
                .as_secs() as i64,
            status: AgentStatus::Idle,
            text: text.to_string(),
        };

        // rkyv Serialization & Ed25519 Signing happening natively in Rust C-Extension!
        let state_bytes = rkyv::to_bytes::<rkyv::rancor::Error>(&state).unwrap();
        let signature = self.signing_key.sign(&state_bytes).to_bytes();

        let envelope = IngressEnvelope {
            intent_path: intent_path.to_string(),
            public_key: self.pub_key_bytes,
            signature,
            state_bytes: state_bytes.into_vec(),
            capability_cert: self.capability_cert_bytes.clone(),
        };

        let serialized_envelope = rkyv::to_bytes::<rkyv::rancor::Error>(&envelope).unwrap();
        let mut final_payload = Vec::new();
        final_payload.extend_from_slice(&(serialized_envelope.len() as u32).to_le_bytes());
        final_payload.extend_from_slice(&serialized_envelope);

        Ok(PyBytes::new(py, &final_payload))
    }

    /// Computes Blake3 32-byte call signature hash over string parameters
    fn hash_call_signatures<'py>(
        &self,
        py: Python<'py>,
        call_inputs: &str,
    ) -> PyResult<Bound<'py, PyBytes>> {
        let mut hasher = blake3::Hasher::new_derive_key("raqim.effect.v1.signature");
        hasher.update(call_inputs.as_bytes());
        let hash_bytes = hasher.finalize();

        Ok(PyBytes::new(py, hash_bytes.as_bytes()))
    }
}

impl RaqimCryptoCore {
    fn mint_capability_certificate(agent_hex: &str, signing_key: &SigningKey) -> Vec<u8> {
        let mut cert = CapabilityCertificate {
            agent_hex: agent_hex.to_string(),
            group_name: "admin_group".to_string(),
            expiration_timestamp: u64::MAX,
            master_signature: Vec::new(),
        };

        let unsigned_bytes =
            postcard::to_allocvec(&cert).expect("Failed to serialize unsigned cert");
        let sig = signing_key.sign(&unsigned_bytes);
        cert.master_signature = sig.to_bytes().to_vec();

        postcard::to_allocvec(&cert).expect("Failed to serialize capability cert")
    }
}

#[pymodule]
fn raqim_core(m: &Bound<'_, PyModule>) -> PyResult<()> {
    m.add_class::<RaqimCryptoCore>()?;
    Ok(())
}
