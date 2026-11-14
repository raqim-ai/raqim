use base64::engine::general_purpose::URL_SAFE_NO_PAD;
use base64::Engine;
use blake3::Hasher;
use ed25519_dalek::{Signer, SigningKey};
use pyo3::prelude::*;
use pyo3::types::PyBytes;
use rand_core::{OsRng, RngCore};
use rkyv::{Archive, Deserialize, Serialize};
use serde::{Deserialize as SerdeDeserialize, Serialize as SerdeSerialize};
use std::time::{SystemTime, UNIX_EPOCH};

#[derive(SerdeSerialize, SerdeDeserialize, Debug, Clone)]
pub struct CapabilityCertificate {
    pub agent_hex: String,
    pub group_name: String,
    pub expiration_timestamp: u64,
    pub master_signature: Vec<u8>,
}

#[repr(u8)]
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
    Intent = 1,
    Outcome = 2,
    Denied = 3,
    System = 4,
}

/// Bit-for-bit identical memory layout to `raqim_core::ExecutionRecord`.
#[derive(
    Archive, Deserialize, Serialize, Debug, PartialEq, Eq, Clone, SerdeDeserialize, SerdeSerialize,
)]
pub struct ExecutionRecord {
    pub tx_id: u128,
    pub agent_id: [u8; 16],
    pub step_ordinal: u64,
    pub timestamp_ms: i64,
    pub namespace: String,
    pub stage: ExecutionStage,
    pub call_signature_hash: [u8; 32],
    pub payload_salt: [u8; 16],
    pub payload_hash: [u8; 32],
    pub payload_bytes: Vec<u8>,
    pub previous_root: [u8; 32],
    pub leaf_hash: [u8; 32],
}

/// Bit-for-bit identical memory layout to `raqim_core::IngressEnvelope`.
#[derive(Archive, Deserialize, Serialize, Debug, Clone)]
pub struct IngressEnvelope {
    pub public_key: [u8; 32],
    pub signature: [u8; 64],
    pub record_bytes: Vec<u8>,
    pub capability_cert: Vec<u8>,
}

#[derive(SerdeSerialize, SerdeDeserialize)]
struct WorkloadClaims {
    sub: String,
    grp: String,
    iat: u64,
    exp: u64,
}

/// Native Rust Cryptographic Core for the Raqim Python SDK.
#[pyclass]
struct RaqimCryptoCore {
    signing_key: SigningKey,

    #[pyo3(get)]
    pub_key_bytes: [u8; 32],

    capability_cert_bytes: Vec<u8>,
}

#[pymethods]
impl RaqimCryptoCore {
    #[new]
    #[pyo3(signature = (pem_path, cert_path=None))]
    fn new(pem_path: &str, cert_path: Option<&str>) -> PyResult<Self> {
        let key_bytes = std::fs::read(pem_path)?;
        let key_array: [u8; 32] = key_bytes.as_slice().try_into().map_err(|_| {
            pyo3::exceptions::PyValueError::new_err("Ed25519 private key must be exactly 32 bytes")
        })?;
        let signing_key = SigningKey::from_bytes(&key_array);
        let pub_key_bytes = signing_key.verifying_key().to_bytes();

        let mut hasher = Hasher::new_derive_key("raqim.agent.v1.identity");
        hasher.update(&pub_key_bytes);
        let mut agent_id_bytes = [0u8; 16];
        hasher.finalize_xof().fill(&mut agent_id_bytes);
        let agent_hex = hex::encode(agent_id_bytes);

        let capability_cert = match cert_path {
            Some(path) if std::path::Path::new(path).exists() => std::fs::read(path)?,
            _ => Self::mint_self_signed_certificate(&agent_hex, "default_group", &signing_key),
        };

        Ok(Self {
            signing_key,
            pub_key_bytes,
            capability_cert_bytes: capability_cert,
        })
    }

    /// Signs arbitrary bytes with the agent's Ed25519 private key and returns the 64-byte signature.
    fn sign_payload<'py>(&self, py: Python<'py>, payload: &[u8]) -> PyResult<Bound<'py, PyBytes>> {
        let signature = self.signing_key.sign(payload).to_bytes();
        Ok(PyBytes::new(py, &signature))
    }

    /// Exposes the serialized `CapabilityCertificate` bytes to the Python runtime.
    #[getter]
    fn capability_cert_bytes<'py>(&self, py: Python<'py>) -> Bound<'py, PyBytes> {
        PyBytes::new(py, &self.capability_cert_bytes)
    }

    /// Packs a Python string record into a length-prefixed, Ed25519-signed `rkyv` `IngressEnvelope`.
    fn generate_tcp_payload<'py>(
        &self,
        py: Python<'py>,
        agent_hex: &str,
        namespace: &str,
        text: &str,
    ) -> PyResult<Bound<'py, PyBytes>> {
        let agent_id_bytes = hex::decode(agent_hex)
            .map_err(|e| pyo3::exceptions::PyValueError::new_err(e.to_string()))?;

        let agent_id_array: [u8; 16] = agent_id_bytes.as_slice().try_into().map_err(|_| {
            pyo3::exceptions::PyValueError::new_err(
                "agent_hex must decode to 16 bytes (32 hex chars)",
            )
        })?;

        let payload_bytes = text.as_bytes().to_vec();
        let mut payload_salt = [0u8; 16];
        OsRng.fill_bytes(&mut payload_salt);
        payload_salt[0] |= 1; // Guarantee non-zero salt for unredacted records

        let mut hasher = Hasher::new_derive_key("raqim.payload.v1");
        hasher.update(&payload_salt);
        hasher.update(&payload_bytes);
        let payload_hash: [u8; 32] = hasher.finalize().into();

        let record = ExecutionRecord {
            tx_id: uuid::Uuid::now_v7().as_u128(),
            agent_id: agent_id_array,
            step_ordinal: 0,
            timestamp_ms: SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap_or_default()
                .as_millis() as i64,
            namespace: namespace.to_string(),
            stage: ExecutionStage::System,
            call_signature_hash: [0u8; 32],
            payload_salt,
            payload_hash,
            payload_bytes,
            previous_root: [0u8; 32],
            leaf_hash: [0u8; 32],
        };

        let record_bytes = rkyv::to_bytes::<rkyv::rancor::Error>(&record)
            .map_err(|e| pyo3::exceptions::PyRuntimeError::new_err(e.to_string()))?;
        let signature = self.signing_key.sign(&record_bytes).to_bytes();

        let envelope = IngressEnvelope {
            public_key: self.pub_key_bytes,
            signature,
            record_bytes: record_bytes.into_vec(),
            capability_cert: self.capability_cert_bytes.clone(),
        };

        let serialized_envelope = rkyv::to_bytes::<rkyv::rancor::Error>(&envelope)
            .map_err(|e| pyo3::exceptions::PyRuntimeError::new_err(e.to_string()))?;

        let mut final_payload = Vec::with_capacity(4 + serialized_envelope.len());
        final_payload.extend_from_slice(&(serialized_envelope.len() as u32).to_le_bytes());
        final_payload.extend_from_slice(&serialized_envelope);

        Ok(PyBytes::new(py, &final_payload))
    }

    /// Computes a domain-separated 32-byte BLAKE3 call signature hash over canonical input strings.
    fn hash_call_signatures<'py>(
        &self,
        py: Python<'py>,
        call_inputs: &str,
    ) -> PyResult<Bound<'py, PyBytes>> {
        let mut hasher = Hasher::new_derive_key("raqim.effect.v1.signature");
        hasher.update(call_inputs.as_bytes());
        let hash_bytes = hasher.finalize();

        Ok(PyBytes::new(py, hash_bytes.as_bytes()))
    }

    /// Static helper: Mints a binary `CapabilityCertificate` signed by the Master Key at `master_key_path`.
    #[staticmethod]
    fn forge_certificate_from_master(
        py: Python<'_>,
        master_key_path: &str,
        agent_hex: &str,
        group_name: &str,
        ttl_seconds: u64,
    ) -> PyResult<Bound<'_, PyBytes>> {
        let master_bytes = std::fs::read(master_key_path)?;
        let master_arr: [u8; 32] = master_bytes.as_slice().try_into().map_err(|_| {
            pyo3::exceptions::PyValueError::new_err("Master key must be exactly 32 bytes")
        })?;
        let master_key = SigningKey::from_bytes(&master_arr);

        let exp = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs()
            .saturating_add(ttl_seconds);

        let mut cert = CapabilityCertificate {
            agent_hex: agent_hex.to_string(),
            group_name: group_name.to_string(),
            expiration_timestamp: exp,
            master_signature: Vec::new(),
        };
        let unsigned = postcard::to_allocvec(&cert)
            .map_err(|e| pyo3::exceptions::PyRuntimeError::new_err(e.to_string()))?;
        cert.master_signature = master_key.sign(&unsigned).to_bytes().to_vec();

        let signed_bytes = postcard::to_allocvec(&cert)
            .map_err(|e| pyo3::exceptions::PyRuntimeError::new_err(e.to_string()))?;
        Ok(PyBytes::new(py, &signed_bytes))
    }

    /// Static helper: Mints an `rqm_v1.<claims_b64>.<sig_b64>` Workload Token signed by `master_key_path`.
    #[staticmethod]
    fn forge_workload_token_from_master(
        master_key_path: &str,
        agent_hex: &str,
        group_name: &str,
        ttl_seconds: u64,
    ) -> PyResult<String> {
        let master_bytes = std::fs::read(master_key_path)?;
        let master_arr: [u8; 32] = master_bytes.as_slice().try_into().map_err(|_| {
            pyo3::exceptions::PyValueError::new_err("Master key must be exactly 32 bytes")
        })?;
        let master_key = SigningKey::from_bytes(&master_arr);

        let now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs();

        let claims = WorkloadClaims {
            sub: agent_hex.to_string(),
            grp: group_name.to_string(),
            iat: now,
            exp: now.saturating_add(ttl_seconds),
        };

        let claims_json = serde_json::to_vec(&claims)
            .map_err(|e| pyo3::exceptions::PyRuntimeError::new_err(e.to_string()))?;
        let claims_b64 = URL_SAFE_NO_PAD.encode(&claims_json);
        let signing_input = format!("rqm_v1.{}", claims_b64);
        let sig = master_key.sign(signing_input.as_bytes());
        let sig_b64 = URL_SAFE_NO_PAD.encode(sig.to_bytes());

        Ok(format!("{}.{}", signing_input, sig_b64))
    }
}

impl RaqimCryptoCore {
    fn mint_self_signed_certificate(
        agent_hex: &str,
        group_name: &str,
        signing_key: &SigningKey,
    ) -> Vec<u8> {
        let mut cert = CapabilityCertificate {
            agent_hex: agent_hex.to_string(),
            group_name: group_name.to_string(),
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
