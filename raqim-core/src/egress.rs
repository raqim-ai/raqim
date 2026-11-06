use axum::body::{Body, Bytes};
use axum::extract::{Path, State};
use axum::http::{HeaderMap, HeaderName, HeaderValue, Method, StatusCode, Uri};
use axum::response::{IntoResponse, Response};
use blake3::Hasher;
use futures::stream::{self, StreamExt};
use serde_json::Value;
use std::sync::Arc;

use crate::api::{ApiError, ApiState};
use crate::effect_store::EffectStore;
use crate::policy::{redact_json_pointers, PolicyError};
use crate::token::{verify_workload_token, TOKEN_PREFIX};

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
    hasher.finalize().into()
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
    if body_bytes.len() > MAX_EGRESS_BODY_BYTES {
        return Err(ApiError::BadRequest(format!(
            "Request body exceeds maximum limit of {} bytes",
            MAX_EGRESS_BODY_BYTES
        )));
    }

    // 1. Authenticate Workload Token (`rqm_v1.<claims>.<sig>`)
    let token_str = extract_workload_token(&headers)?;
    let claims = verify_workload_token(&token_str, &state.policy.master_public_key)?;

    let agent_id_vec = hex::decode(&claims.sub)
        .map_err(|_| ApiError::Unauthorized("Malformed agent_hex in workload token".to_string()))?;
    let agent_id: [u8; 16] = agent_id_vec
        .try_into()
        .map_err(|_| ApiError::Unauthorized("agent_hex must be 16 bytes".to_string()))?;

    // 2. Lookup Named Connector in `RaqimConfig` (Default Deny if unknown)
    let connector_cfg = state
        .config
        .get_connector(&connector_name)
        .ok_or_else(|| {
            ApiError::NotFound(format!(
                "Egress connector '{}' is not configured (Default Deny)",
                connector_name
            ))
        })?
        .clone();

    let clean_subpath = subpath.trim_start_matches('/');
    let normalized_subpath = format!("/{}", clean_subpath);
    let namespace = format!("/egress/{}/{}", connector_name, clean_subpath);
    let query_str = uri.query().unwrap_or("");

    // Extract optional step ordinal (`X-Raqim-Step`) and mode (`X-Raqim-Mode: replay`)
    let step_ordinal: u64 = headers
        .get("x-raqim-step")
        .and_then(|v| v.to_str().ok())
        .and_then(|s| s.parse::<u64>().ok())
        .unwrap_or(0);

    let is_replay_mode = headers
        .get("x-raqim-mode")
        .and_then(|v| v.to_str().ok())
        .map(|m| m.eq_ignore_ascii);

    // 3. Evaluate the 4 Concentric Locks in `PolicyEngine`
    let validated_json_opt = match state.policy.authorize_egress_request(
        &claims.sub,
        &claims.grp,
        method.as_str(),
        &normalized_subpath,
        &namespace,
        &connector_cfg.allowed_routes,
        &body_bytes,
    ) {
        Ok(valid_tree) => valid_tree,
        Err(PolicyError::ForbiddenNamespace(reason)) => {
            // Seal `ExecutionStage::Denied` into the WAL & Merkle DAG before returning 403!
            let denied_sig =
                derive_http_call_signature(method.as_str(), &namespace, query_str, &body_bytes);
            let redacted_req = redact_bytes_for_wal(&body_bytes, &connector_cfg.redact_pointers);
            let _ = state
                .mem_router
                .record_intent_or_denial(
                    agent_id,
                    step_ordinal,
                    denied_sig,
                    &namespace,
                    false,
                    redacted_req,
                )
                .await;
            return Err(ApiError::Forbidden(format!(
                "Policy Interdiction: {}",
                reason
            )));
        }
        Err(other_err) => return Err(ApiError::from(other_err)),
    };

    // 4. Prepare Canonical Outbound Bytes (Defeats JSON key-order & smuggling drift)
    //    and Redacted WAL Request Bytes (Guarantees PII/CVVs never touch disk)
    let (canonical_outbound_body, wal_request_bytes) = match validated_json_opt {
        Some(mut valid_tree) => {
            let outbound = serde_json::to_vec(&valid_tree)
                .map_err(|e| ApiError::InternalServerError(e.to_string()))?;
            if !connector_cfg.redact_pointers.is_empty() {
                redact_json_pointers(&mut valid_tree, &connector_cfg.redact_pointers);
            }
            let wal_bytes = serde_json::to_vec(&valid_tree).unwrap_or_else(|_| outbound.clone());
            (outbound, wal_bytes)
        }
        None => (body_bytes.to_vec(), body_bytes.to_vec()),
    };

    let call_signature_hash =
        derive_http_call_signature(method.as_str(), &namespace, query_str, &wal_request_bytes);

    // 4. Prepare Canonical Outbound Bytes (Defeats JSON key-order & smuggling drift)
    //    and Redacted WAL Request Bytes (Guarantees PII/CVVs never touch disk)
    let (canonical_outbound_body, wal_request_bytes) = match validated_json_opt {
        Some(mut valid_tree) => {
            let outbound = serde_json::to_vec(&valid_tree)
                .map_err(|e| ApiError::InternalServerError(e.to_string()))?;
            if !connector_cfg.redact_pointers.is_empty() {
                redact_json_pointers(&mut valid_tree, &connector_cfg.redact_pointers);
            }
            let wal_bytes = serde_json::to_vec(&valid_tree).unwrap_or_else(|_| outbound.clone());
            (outbound, wal_bytes)
        }
        None => (body_bytes.to_vec(), body_bytes.to_vec()),
    };

    let call_signature_hash =
        derive_http_call_signature(method.as_str(), &namespace, query_str, &wal_request_bytes);

    // 6. Resolve Upstream Secret from Raqim's Vault/Environment BEFORE network call
    let upstream_secret = std::env::var(&connector_cfg.secret_env).map_err(|_| {
        ApiError::InternalServerError(format!(
            "Upstream secret env var '{}' is not set on Raqim daemon",
            connector_cfg.secret_env
        ))
    })?;

    // 7. Commit Write-Ahead `ExecutionStage::Intent` to NVMe WAL (`fdatasync`)!
    let intent_tx_id = state
        .mem_router
        .record_intent_or_denial(
            agent_id,
            step_ordinal,
            call_signature_hash,
            &namespace,
            true,
            wal_request_bytes,
        )
        .await?;
    let intent_tx_hex = format!("{:032x}", intent_tx_id);

    // 8. Build Outbound Request with Injected Secret & Deterministic Idempotency-Key
    let target_url = if query_str.is_empty() {
        format!(
            "{}{}",
            connector_cfg.upstream_url.trim_end_matches('/'),
            normalized_subpath
        )
    } else {
        format!(
            "{}{}?{}",
            connector_cfg.upstream_url.trim_end_matches('/'),
            normalized_subpath,
            query_str
        )
    };

    let reqwest_method =
        reqwest::Method::from_bytes(method.as_str().as_bytes()).unwrap_or(reqwest::Method::POST);
    let client = reqwest::Client::new();
    let mut upstream_req = client.request(reqwest_method, &target_url);

    // Forward safe content headers ONLY (Never forward Authorization, Cookie, Host, or x-api-key)
    for safe_hdr in ["content-type", "accept"] {
        if let Some(val) = headers.get(safe_hdr).and_then(|v| v.to_str().ok()) {
            upstream_req = upstream_req.header(safe_hdr, val);
        }
    }
    // Inject the real upstream credential from Raqim's memory
    let injected_auth_val = format!("{}{}", connector_cfg.auth_prefix, upstream_secret);
    upstream_req = upstream_req.header(&connector_cfg.auth_header, injected_auth_val);

    // Inject deterministic Idempotency-Key bound to the Write-Ahead Intent's UUIDv7!
    if let Some(ref idem_hdr) = connector_cfg.idempotency_header {
        upstream_req = upstream_req.header(idem_hdr.as_str(), &intent_tx_hex);
    }

    if !canonical_outbound_body.is_empty() {
        upstream_req = upstream_req.body(canonical_outbound_body);
    }

    // 9. Dispatch Upstream Request
    let upstream_resp = upstream_req.send().await.map_err(|e| {
        ApiError::InternalServerError(format!(
            "Upstream connection failed (Intent {} marked IN_DOUBT): {}",
            intent_tx_hex, e
        ))
    })?;

    let status_code =
        StatusCode::from_u16(upstream_resp.status().as_u16()).unwrap_or(StatusCode::BAD_GATEWAY);
    let content_type_str = upstream_resp
        .headers()
        .get("content-type")
        .and_then(|v| v.to_str().ok())
        .unwrap_or("application/json")
        .to_string();

    let is_sse_stream = content_type_str
        .to_ascii_lowercase()
        .starts_with("text/event-stream");

    // 10A. Path A: Zero-Stall SSE Streaming Byte-Tee (`text/event-stream`)
    if is_sse_stream && status_code.is_success() {
        let initial_tee_state = SseTeeState {
            inner_stream: upstream_resp.bytes_stream(),
            accumulated: Vec::with_capacity(4096),
            errored: false,
            sealed: false,
            mem_router: state.mem_router.clone(),
            agent_id,
            step_ordinal,
            call_signature_hash,
            namespace,
        };

        let tee_stream = stream::unfold(initial_tee_state, |mut st| async move {
            if st.sealed {
                return None;
            }
            match st.inner_stream.next().await {
                Some(Ok(chunk)) => {
                    if st.accumulated.len() + chunk.len() <= MAX_SSE_CAPTURE_BYTES {
                        st.accumulated.extend_from_slice(&chunk);
                    }
                    Some((Ok::<Bytes, std::io::Error>(chunk), st))
                }
                Some(Err(err)) => {
                    st.errored = true;
                    st.sealed = true;
                    // Stream broke mid-flight -> Do NOT seal Outcome; leave Intent IN_DOUBT!
                    Some((
                        Err(std::io::Error::new(
                            std::io::ErrorKind::Other,
                            err.to_string(),
                        )),
                        st,
                    ))
                }
                None => {
                    st.sealed = true;
                    if !st.errored {
                        let captured = std::mem::take(&mut st.accumulated);
                        let _ = st
                            .mem_router
                            .record_effect(
                                st.agent_id,
                                st.step_ordinal,
                                st.call_signature_hash,
                                captured,
                                &st.namespace,
                            )
                            .await;
                    }
                    None
                }
            }
        });

        let mut resp = Response::new(Body::from_stream(tee_stream));
        *resp.status_mut() = status_code;
        if let Ok(ct_val) = HeaderValue::from_str(&content_type_str) {
            resp.headers_mut()
                .insert(HeaderName::from_static("content-type"), ct_val);
        }
        resp.headers_mut().insert(
            HeaderName::from_static("x-raqim-intent-tx-id"),
            HeaderValue::from_str(&intent_tx_hex).unwrap(),
        );
        return Ok(resp);
    }

    // 10B. Path B: Standard JSON / REST Response
    let resp_bytes = upstream_resp.bytes().await.map_err(|e| {
        ApiError::InternalServerError(format!(
            "Failed reading upstream response body (Intent {} IN_DOUBT): {}",
            intent_tx_hex, e
        ))
    })?;

    // Only seal `ExecutionStage::Outcome` if the upstream call succeeded (2xx)
    let mut final_tx_hex = intent_tx_hex.clone();
    if status_code.is_success() {
        let wal_resp_bytes = redact_bytes_for_wal(&resp_bytes, &connector_cfg.redact_pointers);
        let outcome_tx_id = state
            .mem_router
            .record_effect(
                agent_id,
                step_ordinal,
                call_signature_hash,
                wal_resp_bytes,
                &namespace,
            )
            .await?;
        final_tx_hex = format!("{:032x}", outcome_tx_id);
    }

    let mut resp = Response::new(Body::from(resp_bytes));
    *resp.status_mut() = status_code;
    if let Ok(ct_val) = HeaderValue::from_str(&content_type_str) {
        resp.headers_mut()
            .insert(HeaderName::from_static("content-type"), ct_val);
    }
    resp.headers_mut().insert(
        HeaderName::from_static("x-raqim-tx-id"),
        HeaderValue::from_str(&final_tx_hex).unwrap(),
    );
    resp.headers_mut().insert(
        HeaderName::from_static("x-raqim-replay"),
        HeaderValue::from_static("MISS"),
    );

    Ok(resp)
}
