// =========================================
// RAQIM ED25519 WORKLOAD TOKEN ENGINE
// ==========================================

use std::{
    format,
    time::{SystemTime, UNIX_EPOCH},
};

use base64::{engine::general_purpose::URL_SAFE_NO_PAD, Engine};
use ed25519_dalek::{Signature, Signer, SigningKey, Verifier, VerifyingKey};
use serde::{Deserialize, Serialize};

use crate::policy::PolicyError;

pub const TOKEN_PREFIX: &str = "rqm_v1";

/// Cryptographically bound claims carried inside every agent workload token.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct WorkloadClaims {
    pub sub: String,
    pub grp: String,
    pub iat: u64,
    pub exp: u64,
}

/// Mints a signed `rqm_v1` workload token using the offline/operator Master SigningKey
pub fn mint_workload_token(
    master_signing_key: &SigningKey,
    agent_hex: &str,
    grp_name: &str,
    ttl_sec: u64,
) -> Result<String, anyhow::Error> {
    let now = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs();

    let claims = WorkloadClaims {
        sub: agent_hex.to_string(),
        grp: grp_name.to_string(),
        iat: now,
        exp: now.saturating_add(ttl_sec),
    };

    // Serialize claims to compact json and encode as url-safe base64
    let claims_json = serde_json::to_vec(&claims)?;
    let claim_b64 = URL_SAFE_NO_PAD.encode(&claims_json);

    // Construct the signing output: `rqm_v1.<claims_b64>`
    let signing_input = format!("{}.{}", TOKEN_PREFIX, claim_b64);

    // Sign the ASCII bytes with Ed255519 and encide the 64-byte sig.
    let signature = master_signing_key.sign(signing_input.as_bytes());
    let sig_b64 = URL_SAFE_NO_PAD.encode(signature.to_bytes());

    Ok(format!("{}.{}", signing_input, sig_b64))
}

/// Verifies an `rqm_v1` token against the demon's 'VerifyingKey' (Master Public Key)
/// Enforces cyptographic authenticity and expiration before returning `WorkloadClaims`
pub fn verify_workload_token(
    token_str: &str,
    master_verifying_key: &VerifyingKey,
) -> Result<WorkloadClaims, PolicyError> {
    let trimmed = token_str.trim();

    // Split from the right on the last '.' to separate `rqm_v1.<claims_b64>` from `<sig_b64>`
    let (signing_input, sig_b64) = trimmed.rsplit_once(".").ok_or_else(|| {
        PolicyError::Unauthenticated("Malformed workload token structure".to_string())
    })?;

    // Ensure the prefix is `rqm_v1`
    let claims_b64 = signing_input
        .strip_prefix(&format!("{}.", TOKEN_PREFIX))
        .ok_or_else(|| {
            PolicyError::Unauthenticated("Unsupported or invalid token version prefix".to_string())
        })?;

    // Decode the 64-byte Ed25519 signature
    let sig_vec = URL_SAFE_NO_PAD.decode(sig_b64).map_err(|_| {
        PolicyError::Unauthenticated("Invalid base64 encoding in token signature ".to_string())
    })?;
    let sig_bytes: [u8; 64] = sig_vec.try_into().map_err(|_| {
        PolicyError::Unauthenticated("Token signature must be exactly 64 bytes".to_string())
    })?;

    let signature = Signature::from_bytes(&sig_bytes);

    // 4. CRYPTOGRAPHIC VERIFICATION FOROST (Before touching the json parser)
    master_verifying_key
        .verify(signing_input.as_bytes(), &signature)
        .map_err(|_| {
            PolicyError::Unauthenticated("Invalid Ed25519 signature on workload token".to_string())
        })?;

    // 5. Now that authenticity is proven, decode and pare json claims
    let claim_bytes = URL_SAFE_NO_PAD
        .decode(claims_b64)
        .map_err(|_| PolicyError::Unauthenticated("Invalid base64 in token claims".to_string()))?;
    let claims: WorkloadClaims = serde_json::from_slice(&claim_bytes)
        .map_err(|_| PolicyError::Unauthenticated("Malformed JSON in token claims".to_string()))?;

    // Enfirce token TTL expiration
    let now = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs();

    if now > claims.exp {
        return Err(PolicyError::Unauthenticated(
            "Workload token has expired".to_string(),
        ));
    }

    Ok(claims)
}
