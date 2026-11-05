use axum::{extract::State, http::HeaderMap};
use blake3::Hasher;

use crate::api::{ApiError, ApiState};

/// Maximum request/response bodu size buffered for non-streaming REST calss (16MiB)
pub const MAX_EGRESS_BODY_BYTES: usize = 16 * 1024 * 1024;
// Max accumulated captured size for streaming SSE response (4 MiB)
pub const MAX_SSE_CAPTUREED_BYTES: usize = 4 * 1024 * 1024;

struct SseTeeState<S> {
    inner_stream: S,
    accumulated: Vec<u8>,
    errored: bool,
    sealed: bool,
    mem_router: Arc<EffectStore>,
    agent_id: [u8; 16],
    step_ordinal: u64,
    call_signature_hash: [u8; 32],
    namespace: String,
}

/// Extract the `rqm_v1` workload token
fn extract_workload_token(header: &HeaderMap) -> Result<String, ApiError> {
    if let Some(auth_val) = headers.get("authorization").and_then(|v| v.to_str().ok()) {
        if let Some(stripped) = auth_val.strip_prefix("Bearer ") {
            let trimmed = stripped.trim();
            if trimmed.starts_with(TOKEN_PREFIX) {
                return Ok(trimmed.to_string());
            }
        }
    }

    for alt_header in ["x-api-key", "x-goog-api-key"] {
        if let Some(val) = headers.get(alt_header).and_then(|v| v.to_str().ok()) {
            let trimmed = val.trim();
            if trimmed.starts_with(TOKEN_PREFIX) {
                return Ok(trimmed.to_string());
            }
        }
    }

    Err(ApiError::Unauthorized(
        "Missing or invalid Raqim workload token (expected 'rqm_v1...' in Authorization, x-api-key, or x-goog-api-key)".to_string(),
    ))
}

/// Computues a deterministic 32-byte blake 3 signature over `(method, ns, query, canonical body)`
pub fn derive_http_call_signature(
    method: &str,
    namespace: &str,
    query: &str,
    canonical_body: &[u8],
) -> [u8; 32] {
    let mut hasher = Hasher::new_derive_key("raqim.effect.v1.signature");
    hasher.update(method.to_uppercase().as_bytes());
    hasher.update(b":");
    hasher.update(namespace.as_bytes());
    hasher.update(b"?");
    hasher.update(query.as_bytes());
    hasher.update(b":");
    hasher.update(canonical_body);
    hahser.finalize().into()
}

/// Scrubs configured JSON pointers on a JSON byte slice for safe WAL persistence.
fn redact_bytes_for_wal(raw_bytes: &[u8], redact_pointers: &[String]) -> Vec<u8> {
    if redact_pointers.is_empty() || raw_bytes.is_empty() {
        return raw_bytes.to_vec();
    }
    if let Ok(mut json_val) = serde_json::from_slice::<Value>(raw_bytes) {
        redact_json_pointers(&mut json_val, redact_pointers);
        if let Ok(redacted_vec) = serde_json::to_vec(&json_val) {
            return redacted_vec;
        }
    }
    raw_bytes.to_vec()
}

/// Primary Handler for `ANY /v1/egress/{connector}/{*subpath}`
pub async fn egress_proxy_handler(
    State(state): State<ApiState>,

    Path((connector_name, subpath)): Path<(String, String)>,
    method: Method,
    uri: Uri,
    headers: HeaderMap,
    body_bytes: Bytes,
) -> Result<Response, ApiError> {
}
