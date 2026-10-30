use axum::extract::{FromRef, Path, Query};
use axum::http::header::AUTHORIZATION;
use axum::response::{IntoResponse, Response};
use axum::{
    extract::{FromRequestParts, State},
    http::{request::Parts, StatusCode},
    routing::{get, post},
    Json,
};
use base64::Engine;
use tokio::sync::watch;
use tower_http::catch_panic::CatchPanicLayer;

use axum::body::Bytes;
use serde_json::{json, Value};
use std::time::{SystemTime, UNIX_EPOCH};
use std::{eprintln, format, println};

use serde::{Deserialize, Serialize};
use std::result::Result::{Err, Ok};
use std::sync::Arc;
use tokio::sync::broadcast::Sender;

use crate::cold_store::ColdStore;
use crate::compactor::WalCompactor;
use crate::merkle::{InclusionProof, MerkleEngine};
use crate::otel::{self, OtelExportRequest};
use crate::policy::{PolicyError, QuarantineRecord};
use crate::registry::SwarmRegistry;
use crate::wal::WalEngine;
use crate::{config::RaqimConfig, effect_store::EffectStore, policy::PolicyEngine};
use crate::{execute_raqim_cascade, ExecutionRecord, IngressEnvelope, RecentRecord, SystemEvent};

// Strongly typed api error system (Zero-Panic Guarantee)
#[derive(Debug)]
pub enum ApiError {
    BadRequest(String),
    Unauthorized(String),
    Forbidden(String),

    NotFound(String),
    RateLimitExceeded(String),
    InternalServerError(String),
}

impl From<PolicyError> for ApiError {
    fn from(err: PolicyError) -> Self {
        match err {
            PolicyError::Unauthenticated(m) => ApiError::Unauthorized(m),
            PolicyError::Quarantined(m) => ApiError::Forbidden(m),
            PolicyError::ForbiddenNamespace(m) => ApiError::Forbidden(m),
            PolicyError::RateLimited(m) => ApiError::RateLimitExceeded(m),
        }
    }
}

impl From<anyhow::Error> for ApiError {
    fn from(err: anyhow::Error) -> Self {
        ApiError::InternalServerError(err.to_string())
    }
}

impl IntoResponse for ApiError {
    fn into_response(self) -> Response {
        let (status, error_code, message) = match self {
            ApiError::BadRequest(msg) => (StatusCode::BAD_REQUEST, "BAD_REQUEST", msg),
            ApiError::Unauthorized(msg) => (StatusCode::UNAUTHORIZED, "UNAUTHORIZED", msg),
            ApiError::Forbidden(msg) => (StatusCode::FORBIDDEN, "FORBIDDEN", msg),
            ApiError::NotFound(msg) => (StatusCode::NOT_FOUND, "NOT_FOUND", msg),
            ApiError::RateLimitExceeded(msg) => {
                (StatusCode::TOO_MANY_REQUESTS, "RATE_LIMIT_EXCEEDED", msg)
            }
            ApiError::InternalServerError(msg) => {
                (StatusCode::INTERNAL_SERVER_ERROR, "INTERNAL_ERROR", msg)
            }
        };

        let body = Json(json!({

            "success": false,
            "error_code": error_code,
            "message": message

        }));

        (status, body).into_response()
    }
}

#[derive(Clone)]
pub struct ApiState {
    pub config: Arc<RaqimConfig>,
    pub effect_store: Arc<EffectStore>,
    pub merkle: Arc<MerkleEngine>,

    pub policy: Arc<PolicyEngine>,
    pub wal: Arc<WalEngine>,
    pub cold_store: Arc<dyn ColdStore>,

    pub event_tx: Sender<SystemEvent>,
    pub swarm_registry: Arc<SwarmRegistry>,
    pub admin_secret: String,

    pub pause_tx: Arc<watch::Sender<bool>>,
    pub compactor: Arc<WalCompactor>,
}

/// Strict Operator Authentication Extractor.
#[derive(Clone, Debug)]
pub struct ValidatedIdentity {
    pub tenant_id: String,
    pub is_admin: bool,
}

impl<S> FromRequestParts<S> for ValidatedIdentity
where
    S: Send + Sync,
{
    type Rejection = ApiError;

    async fn from_request_parts(parts: &mut Parts, state: &S) -> Result<Self, Self::Rejection> {
        let api_state = ApiState::from_ref(state);

        // If an Authorization header is provided,  validate it strictky via contant-time comparison
        if let Some(auth_header) = parts
            .headers
            .get(AUTHORIZATION)
            .and_then(|val| val.to_str().ok())
        {
            let provided_token = auth_header.strip_prefix("Bearer ").ok_or_else(|| {
                ApiError::Unauthorized(
                    "Invalid auth scheme. Expected 'Authorization: Bearer <token>' ".to_string(),
                )
            })?;

            if api_state.admin_secret.is_empty() {
                return Err(ApiError::InternalServerError(
                    "Admin secret is not initialized on daemon".to_string(),
                ));
            }

            if constant_time_compare(provided_token.as_bytes(), api_state.admin_secret.as_bytes()) {
                return Ok(ValidatedIdentity {
                    tenant_id: api_state.config.tenant_id.clone(),
                    is_admin: true,
                });
            } else {
                return Err(ApiError::Unauthorized(
                    "Access Denied: Invalid Administrative Bearer Token ".to_string(),
                ));
            }
        }

        Err(ApiError::Unauthorized(
            "Missing 'Authorization: Bearer <token>' header for admin plane".to_string(),
        ))
    }
}

#[derive(Deserialize)]
pub struct RecentRecordParams {
    pub limit: Option<usize>,
}

pub async fn recent_records_endpoint(
    _auth: ValidatedIdentity,
    State(state): State<ApiState>,
    Query(params): Query<RecentRecordParams>,
) -> Result<Json<Vec<RecentRecord>>, ApiError> {
    let limit = params.limit.unwrap_or(50).min(500);

    // 1. Gather records from active RAM buffers
    let ram_records = state.merkle.get_recent_record_ram(limit);

    // 2. Gather records from WAL disk
    let wal_records = state
        .wal
        .read_recent_wal_records(&state.config.wal_path, limit);

    // 3. Merge and deduplicate by tx_id
    let mut seen = std::collections::HashSet::new();
    let mut all_records = Vec::new();

    for t in ram_records {
        if seen.insert(t.tx_id.clone()) {
            all_records.push(t);
        }
    }

    for t in wal_records.into_iter().rev() {
        if seen.insert(t.tx_id.clone()) {
            all_records.push(t);
        }
    }

    // Sort chronologically (oldest first so UI appends cleanly)
    all_records.sort_by(|a, b| a.timestamp.cmp(&b.timestamp));
    if all_records.len() > limit {
        all_records = all_records.split_off(all_records.len() - limit);
    }

    Ok(Json(all_records))
}

pub async fn active_quarantine_endpoint(
    _auth: ValidatedIdentity,
    State(state): State<ApiState>,
) -> Json<Vec<QuarantineRecord>> {
    let mut quarantined_agents = Vec::new();

    // Iterate over Dashmap Shards safely.
    for entry in state.policy.quarantine_blocklist.iter() {
        quarantined_agents.push(entry.value().clone());
    }

    // Sort by most recent first
    quarantined_agents.sort_by(|a, b| b.timestamp.cmp(&a.timestamp));

    Json(quarantined_agents)
}

#[derive(Deserialize)]
struct LiftQuarantinePagyload {
    agent_hex: String,
}

async fn lift_quarantine_handler(
    _auth: ValidatedIdentity,
    State(state): State<ApiState>,
    Json(payload): Json<LiftQuarantinePagyload>,
) -> Result<Json<Value>, ApiError> {
    // Unfreeze the Agent (Remove from RAM)
    if state
        .policy
        .quarantine_blocklist
        .remove(&payload.agent_hex)
        .is_some()
    {
        // Append to durable control journal
        let _ = crate::checkpoint::CheckpointEngine::append_control_mutation(
            std::path::Path::new(&state.config.control_journal_path),
            &crate::checkpoint::ControlMutation::LiftQuarantine {
                agent_hex: payload.agent_hex.clone(),
            },
        );

        // Also update the Ram process table so the Topology page knows it's alive again.
        state
            .swarm_registry
            .touch_agent(&payload.agent_hex, "Unknown", "Active", "Unknown");

        println!(
            "[POLICY] Quarantine lifted for agent: {}",
            payload.agent_hex
        );

        Ok(Json(json!({
            "success": true,
            "message": "Quarantine lifted"
        })))
    } else {
        Err(ApiError::NotFound(format!(
            "Agent {} not in quarantine",
            payload.agent_hex
        )))
    }
}

pub async fn in_doubt_intents_endpoint(
    _auth: ValidatedIdentity,
    State(state): State<ApiState>,
) -> Json<Vec<ExecutionRecord>> {
    Json(state.effect_store.get_in_doubt_intents())
}

pub async fn redact_record_endpoint(
    _auth: ValidatedIdentity,
    State(state): State<ApiState>,
    Path(tx_param): Path<String>,
) -> Result<Json<Value>, ApiError> {
    let clean_hex = tx_param.trim_start_matches("0x");
    let tx_id = u128::from_str_radix(clean_hex, 16)
        .or_else(|_| clean_hex.parse::<u128>())
        .map_err(|_| ApiError::BadRequest("Invalid tx_id hex".to_string()))?;

    let redacted = state.effect_store.redact_record(tx_id).await?;
    if redacted {
        Ok(Json(json!( {
            "success": true, 
            "tx_id_hex": format!("{:032x}", tx_id),
            "message": "PII payload and salt cryptographically shredded (Merkle proof preserved) "
        } )))
    } else {
        Err(ApiError::NotFound(format!(
            "Transaction {:032x} not found",
            tx_id
        )))
    }
}

/// Zero-copy Binary Ingress Endpoint ("Point /v1/swarm/ingress")         
pub async fn http_ingress_endpoint(
    State(state): State<ApiState>,
    body: Bytes,
) -> Result<Json<Value>, ApiError> {
    // Zero copy access the IngressEnvelope
    let archived_ingress = rkyv::access::<
        <IngressEnvelope as rkyv::Archive>::Archived,
        rkyv::rancor::Error,
    >(&body)
    .map_err(|e| ApiError::BadRequest(format!("Malformed IngressEnvelope memory layout: {}", e)))?;

    let record_bytes = archived_ingress.record_bytes.as_slice();

    let mut aligned_buf = rkyv::util::AlignedVec::<16>::with_capacity(record_bytes.len());
    aligned_buf.extend_from_slice(record_bytes);

    let archived_rec = rkyv::access::<
        <ExecutionRecord as rkyv::Archive>::Archived,
        rkyv::rancor::Error,
    >(&aligned_buf)
    .map_err(|e| ApiError::BadRequest(format!("Malformed ExecutionRecord: {}", e)))?;

    let signed_namespace = archived_rec.namespace.as_str();

    let agent_pub_key: [u8; 32] = archived_ingress
        .public_key
        .try_into()
        .map_err(|_| ApiError::BadRequest("Public key must be exactly 32 bytes".to_string()))?;

    // O(1) policy Policy Check.
    let mut packet_sig = [0u8; 64];
    if archived_ingress.signature.len() != 64 {
        return Err(ApiError::BadRequest(
            "Signature must be exactly 64 bytes".to_string(),
        ));
    }
    packet_sig.copy_from_slice(archived_ingress.signature.as_slice());

    let (agent_hex, group_name) = state
        .policy
        .verify_session_lineage(&archived_ingress.capability_cert.as_slice(), &agent_pub_key)
        .map_err(|e| ApiError::Unauthorized(format!("Lineage Verification failed: {}", e)))?;

    let packet_ts_sec = (archived_rec.timestamp.to_native() / 1000) as i64;
    state
        .policy
        .authorize_packet_fast(
            agent_hex.as_str(),
            group_name.as_str(),
            &agent_pub_key,
            &record_bytes,
            &packet_sig,
            &signed_namespace,
            packet_ts_sec,
        )
        .map_err(|e| ApiError::Forbidden(format!("policy Interdiction: {}", e)))?;

    let tx_id = execute_raqim_cascade(
        archived_rec,
        state.merkle.clone(),
        state.wal.clone(),
        state.event_tx.clone(),
    )
    .await
    .map_err(|e| ApiError::InternalServerError(e.to_string()))?;

    Ok(Json(
        json!({"success": true, "tx_id_hex": format!("{:032x}", tx_id) }),
    ))
}

#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct TimelineNode {
    pub tx_id: u128,
    pub timestamp: String,
    pub agent_status: String,
    pub payload_preview: String,
}

pub async fn fetch_agent_timeline(
    _auth: ValidatedIdentity,
    State(state): State<ApiState>,
    Path(agent_hex): Path<String>,
) -> Result<Json<Vec<TimelineNode>>, ApiError> {
    // The scatter: Let the engines do their native work
    let cold_future = state.cold_store.fetch_historical_timeline(&agent_hex);
    let wal_future = async {
        state
            .wal
            .fetch_hot_timeline(&agent_hex, &state.config.wal_path)
    };

    let (cold_res, wal_res) = tokio::join!(cold_future, wal_future);

    // The Gather
    let mut nodes = wal_res.unwrap_or_default();
    if let Ok(mut cold_res) = cold_res {
        nodes.append(&mut cold_res)
    }

    nodes.sort_by(|a, b| a.tx_id.cmp(&b.tx_id));
    nodes.dedup_by_key(|n| n.tx_id);

    Ok(Json(nodes))
}

pub async fn toggle_ingress_endpoint(
    _auth: ValidatedIdentity,
    State(state): State<ApiState>,
) -> Result<Json<serde_json::Value>, ApiError> {
    let prev = *state.pause_tx.borrow();
    let new_state = !prev;

    // Broadcasts state change across all worker tasks
    let _ = state.pause_tx.send(new_state);

    println!(
        "[SYSTEM] Ingress flow control changed: {} ",
        if new_state { "PAUSED " } else { "ACTIVE" }
    );

    Ok(Json(serde_json::json!({
        "success": true,
        "ingress_paused": new_state
    })))
}

#[derive(Serialize, Clone, Debug)]
pub struct GroupPolicyTelemetry {
    pub group_name: String,
    pub allowed_namspace: Vec<String>,
    pub blocked_namespace: Vec<String>,
    pub max_tps: u64,
    pub burst_capacity: u64,
}

#[derive(Serialize)]
pub struct PolicyMetricsData {
    pub total_quarantined: usize,
    pub recent_interdictions: usize,
    pub signature_spoofs: usize,
    pub namespace_breaches: usize,
    pub rate_limit_blocks: usize,
    pub active_policies: Vec<GroupPolicyTelemetry>,
}

pub async fn policy_metrics_endpoint(
    _auth: ValidatedIdentity,
    State(state): State<ApiState>,
) -> Result<Json<PolicyMetricsData>, ApiError> {
    let mut metrics = PolicyMetricsData {
        total_quarantined: 0,
        recent_interdictions: 0,
        signature_spoofs: 0,
        namespace_breaches: 0,
        rate_limit_blocks: 0,
        active_policies: Vec::new(),
    };

    let now = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs();
    let ten_minutes_ago = now.saturating_sub(600);

    // Safely iterate the dashmap shards.
    for entry in state.policy.quarantine_blocklist.iter() {
        metrics.total_quarantined += 1;
        let record = entry.value();

        // Check if it happens 10 minutes ago
        if record.timestamp >= ten_minutes_ago {
            metrics.recent_interdictions += 1;
        }

        // Tally strict violation types
        match record.violation_type.as_str() {
            "CRYPTO_SPOOF" => metrics.signature_spoofs += 1,
            "NAMESPACE_BREACH" => metrics.namespace_breaches += 1,
            "SUSTAINED_RATE_FLOOD" | "RATE_LIMIT_EXCEEDED" => metrics.rate_limit_blocks += 1,
            _ => {}
        }
    }

    // Query live Atomic token bucket across active policies
    let policies_guard = state.policy.group_policies.read();
    for (group_name, policy) in policies_guard.iter() {
        metrics.active_policies.push(GroupPolicyTelemetry {
            group_name: group_name.clone(),
            allowed_namspace: policy.allowed_namespaces.clone(),
            blocked_namespace: policy.blocked_namespaces.clone(),
            max_tps: policy.max_tps,
            burst_capacity: policy.burst_capacity,
        });
    }

    Ok(Json(metrics))
}

#[derive(serde::Deserialize)]
pub struct MintRequest {
    pub agent_hex: String,
    pub group: String,
}

/// Maps to `raqim cluster info`
pub async fn cluster_info_endpoint(
    _auth: ValidatedIdentity,
    State(state): State<ApiState>,
) -> Result<Json<Value>, ApiError> {
    let pending_wal_items = state.wal.get_pending_count().await;
    let wal_size = std::fs::metadata(&state.config.wal_path)
        .map(|m| m.len())
        .unwrap_or(0);

    let highest_tx = state.merkle.get_latest_tx_id();
    let cold_records = state.cold_store.get_total_record_count().await;

    let payload = json!({
        "highest_tx_id": format!("0x{:032x}", highest_tx),
        "wal_bytes": wal_size,
        "wal_size_mb": (wal_size as f64 ) / (1024.0 * 1024.0),
        "buffer_load": pending_wal_items,
        "cold_records": cold_records,
        "in_doubt_intents": state.effect_store.get_in_doubt_intents().len()
    });

    Ok(Json(payload))
}

#[derive(Deserialize)]
pub struct RecordEffectRequest {
    pub agent_hex: String,
    pub step_ordinal: u64,
    pub call_signature_hex: String,
    pub output_payload_base64: String,
    pub namespace: String,

    // Cryptographic perimeter
    pub timestamp: i64,
    pub public_key_hex: String,
    pub capability_cert_hex: String,
    pub signature_hex: String,
}

#[derive(Serialize)]
pub struct RecordEffectResponse {
    pub success: bool,
    pub tx_id_hex: String,
    pub is_forked_branch: bool,
}

#[derive(Deserialize)]
pub struct GetEffectRequest {
    pub agent_hex: String,
    pub step_ordinal: u64,
    pub call_signature_hex: String,
}

#[derive(Serialize)]
pub struct GetEffectResponse {
    pub found: bool,
    pub output_payload_base64: Option<String>,
    pub timestamp: Option<i64>,
}

#[derive(Deserialize, Debug)]
pub struct PreflightEffectRequest {
    pub agent_hex: String,
    pub step_ordinal: u64,
    pub call_signature_hex: String,
    pub namespace: String,

    // Cryptographic perimeter
    pub timestamp: i64,
    pub public_key_hex: String,
    pub capability_cert_hex: String,
    pub signature_hex: String,
}

#[derive(Serialize)]
pub struct PreflightEffectResponse {
    pub allowed: bool,
    pub agent_hex: String,
    pub namespace: String,
    pub intent_tx_id_hex: String,
}

/// Pre-flight Policy Audit & Write-Ahead Intent Recorder (`POST /v1/effect/preflight`)
/// 1. Verifies lineage & Ed25519 signature First
/// 2. If denied by namespace policy: commits `ExecutionStage::Denied` to WAL & Merkle tree, then returns 403
/// 3. If allowed: commits `ExecutionStage::Intent` to WAL & `pending_intents` BEFORE returning 200 OK
pub async fn preflight_effect_handler(
    State(state): State<ApiState>,
    Json(payload): Json<PreflightEffectRequest>,
) -> Result<Json<PreflightEffectResponse>, ApiError> {
    let agent_id_bytes = hex::decode(&payload.agent_hex)
        .map_err(|_| ApiError::BadRequest("Invalid agent_hex format".to_string()))?;
    let agent_id: [u8; 16] = agent_id_bytes
        .try_into()
        .map_err(|_| ApiError::BadRequest("agent_hex must be 16 bytes".to_string()))?;

    let pub_key_bytes = hex::decode(&payload.public_key_hex)
        .map_err(|_| ApiError::BadRequest("Invalid pub_key_hex format".to_string()))?;

    let pub_key: [u8; 32] = pub_key_bytes
        .try_into()
        .map_err(|_| ApiError::BadRequest("pub_key must be exactly 32 bytes".to_string()))?;

    let signature_bytes = hex::decode(&payload.signature_hex)
        .map_err(|_| ApiError::BadRequest("Invalid signature format".to_string()))?;

    let signature: [u8; 64] = signature_bytes
        .try_into()
        .map_err(|_| ApiError::BadRequest("signature must be exactly 64 bytes".to_string()))?;

    let capability_cert = hex::decode(&payload.capability_cert_hex)
        .map_err(|_| ApiError::BadRequest("Invalid capability_cert_hex format".to_string()))?;

    let call_signature_bytes = hex::decode(&payload.call_signature_hex)
        .map_err(|_| ApiError::BadRequest("Invalid call_signature_hex format".to_string()))?;
    let call_signature_hash: [u8; 32] = call_signature_bytes
        .try_into()
        .map_err(|_| ApiError::BadRequest("call_signature_hex must be 32 bytes".to_string()))?;

    let (verified_agent_hex, group_name) = state
        .policy
        .verify_session_lineage(&capability_cert, &pub_key)
        .map_err(|e| ApiError::Unauthorized(format!("Lineage rejection: {}", e)))?;

    if verified_agent_hex != payload.agent_hex {
        return Err(ApiError::Forbidden(
            "Security violation: Certificate identity mismatch with caller".to_string(),
        ));
    }

    if state.policy.is_quarantined(&payload.agent_hex) {
        return Err(ApiError::Forbidden(
            "Agent is actively quarantined by PolicyEngine".to_string(),
        ));
    }

    let canonical_attestation = format!(
        "preflight:{}:{}:{}:{}",
        payload.namespace, payload.step_ordinal, payload.call_signature_hex, payload.timestamp
    );

    match state.policy.authorize_packet_fast(
        &payload.agent_hex,
        &group_name,
        &pub_key,
        canonical_attestation.as_bytes(),
        &signature,
        &payload.namespace,
        payload.timestamp,
    ) {
        Ok(()) => {
            // Write-Ahead Intent committed to NVMe WAL before function executes!
            let intent_tx_id = state
                .effect_store
                .record_intent_or_denial(
                    agent_id,
                    payload.step_ordinal,
                    call_signature_hash,
                    &payload.namespace,
                    true,
                    b"PREFLIGHT_INTENT_AUTHORIZED".to_vec(),
                )
                .await?;

            Ok(Json(PreflightEffectResponse {
                allowed: true,
                agent_hex: payload.agent_hex,
                namespace: payload.namespace,
                intent_tx_id_hex: format!("{:032x}", intent_tx_id),
            }))
        }

        Err(PolicyError::ForbiddenNamespace(reason)) => {
            // Authenticated policy violation: seal `ExecutionStage::Denied` into wal and merkle engine
            let _ = state
                .effect_store
                .record_intent_or_denial(
                    agent_id,
                    payload.step_ordinal,
                    call_signature_hash,
                    &payload.namespace,
                    false,
                    reason.as_bytes().to_vec(),
                )
                .await;

            Err(ApiError::Forbidden(format!(
                "Policy Interdiction: {}",
                reason
            )))
        }

        Err(other_err) => Err(ApiError::from(other_err)),
    }
}

/// Records live side-effect into WAL + Merkle DAG
pub async fn record_effect_handler(
    State(state): State<ApiState>,
    Json(payload): Json<RecordEffectRequest>,
) -> Result<Json<RecordEffectResponse>, ApiError> {
    let agent_id_bytes = hex::decode(payload.agent_hex.clone())
        .map_err(|_| ApiError::BadRequest("Invalid agent_hex format".to_string()))?;

    let agent_id: [u8; 16] = agent_id_bytes
        .try_into()
        .map_err(|_| ApiError::BadRequest("agent_hex must be exactly 16 bytes".to_string()))?;

    let pub_key_bytes = hex::decode(payload.public_key_hex.clone())
        .map_err(|_| ApiError::BadRequest("Invalid pub_key_hex format".to_string()))?;

    let pub_key: [u8; 32] = pub_key_bytes
        .try_into()
        .map_err(|_| ApiError::BadRequest("pub_key must be exactly 32 bytes".to_string()))?;

    let signature_bytes = hex::decode(payload.signature_hex.clone())
        .map_err(|_| ApiError::BadRequest("Invalid signature format".to_string()))?;

    let signature: [u8; 64] = signature_bytes
        .try_into()
        .map_err(|_| ApiError::BadRequest("signature must be exactly 64 bytes".to_string()))?;

    let capability_cert = hex::decode(payload.capability_cert_hex)
        .map_err(|_| ApiError::BadRequest("Invalid capability_cert_hex format".to_string()))?;

    let call_signature_bytes = hex::decode(payload.call_signature_hex.clone())
        .map_err(|_| ApiError::BadRequest("Invalid call_signature_hex format".to_string()))?;
    let call_signature_hash: [u8; 32] = call_signature_bytes.try_into().map_err(|_| {
        ApiError::BadRequest("call_signature_hex must be eactly 32 bytes".to_string())
    })?;

    let output_payload = base64::engine::general_purpose::STANDARD
        .decode(&payload.output_payload_base64)
        .map_err(|_| ApiError::BadRequest("Invalid base64 payload".to_string()))?;

    // policy barrier 1: Lineage Verification
    let (verified_agent_hex, group_name) = state
        .policy
        .verify_session_lineage(&capability_cert, &pub_key)
        .map_err(|e| ApiError::Unauthorized(format!("policy lineage rejection: {}", e)))?;

    if verified_agent_hex != payload.agent_hex {
        return Err(ApiError::Forbidden(
            " Security violation: Certificate identity mismatch with caller ".to_string(),
        ));
    }

    let canonical_attestation = format!(
        "{}:{}:{}:{}:{}",
        payload.namespace.clone(),
        payload.step_ordinal,
        payload.call_signature_hex.clone(),
        payload.output_payload_base64,
        payload.timestamp
    );

    // policy Barrier 2: Fast-path policy audit
    state
        .policy
        .authorize_packet_fast(
            &payload.agent_hex,
            &group_name,
            &pub_key,
            canonical_attestation.as_bytes(),
            &signature,
            &payload.namespace,
            payload.timestamp,
        )
        .map_err(|e| ApiError::Forbidden(format!("policy Interdiction: {}", e)))?;

    let is_forked = payload.namespace.starts_with("phantom_");

    match state
        .effect_store
        .record_effect(
            agent_id,
            payload.step_ordinal,
            call_signature_hash,
            output_payload,
            &payload.namespace,
        )
        .await
    {
        Ok(tx_id) => Ok(Json(RecordEffectResponse {
            success: true,
            tx_id_hex: format!("{:032x}", tx_id),
            is_forked_branch: is_forked,
        })),

        Err(e) => Err(ApiError::InternalServerError(format!(
            "Failed to record effect: {}",
            e
        ))),
    }
}

/// Fetches a recoorded effect for deterministic replay
pub async fn get_effect_handler(
    State(state): State<ApiState>,
    Json(payload): Json<GetEffectRequest>,
) -> Result<Json<GetEffectResponse>, ApiError> {
    let agent_id_bytes = hex::decode(payload.agent_hex)
        .map_err(|_| ApiError::BadRequest("Invalid agent_hex format".to_string()))?;
    let agent_id: [u8; 16] = agent_id_bytes
        .try_into()
        .map_err(|_| ApiError::BadRequest("agent_hex must be exactly 16 bytes".to_string()))?;

    let call_signature_bytes = hex::decode(payload.call_signature_hex)
        .map_err(|_| ApiError::BadRequest("Invalid call_signature_hex format".to_string()))?;
    let call_signature_hash: [u8; 32] = call_signature_bytes.try_into().map_err(|_| {
        ApiError::BadRequest("call_signature_hex must be exactly 32 bytes ".to_string())
    })?;

    match state
        .effect_store
        .get_effect(&agent_id, payload.step_ordinal, &call_signature_hash)
    {
        Some(record) => {
            let b64_payload =
                base64::engine::general_purpose::STANDARD.encode(&record.payload_bytes);

            Ok(Json(GetEffectResponse {
                found: true,
                output_payload_base64: Some(b64_payload),
                timestamp: Some(record.timestamp),
            }))
        }

        None => Ok(Json(GetEffectResponse {
            found: false,
            output_payload_base64: None,
            timestamp: None,
        })),
    }
}

#[derive(Serialize)]
pub struct StateProofResponse {
    pub success: bool,
    pub proof: Option<InclusionProof>,
    pub message: String,
}

/// Generate an 0(log N) Merkle Inclusion Proof for any tx_id
pub async fn get_state_proof_handler(
    State(state): State<ApiState>,
    Path(tx_param): Path<String>,
) -> Result<Json<StateProofResponse>, ApiError> {
    let clean_hex = tx_param.trim_start_matches("0x");
    // Parse u128 UUIDv7
    let tx_id = u128::from_str_radix(clean_hex, 16)
        .or_else(|_| clean_hex.parse::<u128>())
        .map_err(|_| {
            ApiError::BadRequest("tx_id must be a valid 32-character hex string".to_string())
        })?;

    // Query MerkleEngine for 0(log N) Inclusion proof
    match state.merkle.generate_proof_for_tx(tx_id) {
        Some(proof) => Ok(Json(StateProofResponse {
            success: true,
            proof: Some(proof.clone()),
            message: if proof.is_active_buffer {
                "Proof generated against active workspace buffer".to_string()
            } else {
                "proof generated against immutable Merkle Batch archive.".to_string()
            },
        })),

        None => Ok(Json(StateProofResponse {
            success: false,
            proof: None,
            message: format!("Transaction ID {:032x} not found.", tx_id),
        })),
    }
}

pub async fn trigger_compaction_endpoint(
    State(state): State<ApiState>,
) -> Result<Json<serde_json::Value>, ApiError> {
    println!("[ADMIN] Manual WAL compaction requested via Admin Socket...");

    let compactor = state.compactor.clone();

    tokio::spawn(async move {
        match compactor.trigger_safe_compaction().await {
            Ok(count) => {
                println!(
                    "[ADMIN SUCCESS] Background compaction complete. Archived {} records to coldDB.",
                    count
                )
            }
            Err(e) => {
                eprintln!(
                    "[ADMIN ERROR] Background compaction encountered an error: {}",
                    e
                );
            }
        }
    });

    Ok(Json(serde_json::json!({
        "success": "ACCEPTED",
        "message": "WAL rotation and 2PC ColdStore compaction initiated."
    })))
}

pub async fn export_agent_timeline_otel(
    _auth: ValidatedIdentity,
    State(state): State<ApiState>,
    Path(agent_hex): Path<String>,
) -> Result<Json<OtelExportRequest>, ApiError> {
    // Scatter-gather historical timeline from coldDB and WAL
    let cold_future = state.cold_store.fetch_historical_timeline(&agent_hex);
    let wal_future = async {
        state
            .wal
            .fetch_hot_timeline(&agent_hex, &state.config.wal_path)
    };

    let (cold_res, wal_res) = tokio::join!(cold_future, wal_future);

    let mut nodes = wal_res.unwrap_or_default();
    if let Ok(mut cold_res) = cold_res {
        nodes.append(&mut cold_res);
    }

    if nodes.is_empty() {
        return Err(ApiError::NotFound(format!(
            "No recorded timeline for agent {}",
            agent_hex
        )));
    }

    // fetch active merkle root
    let active_root = state
        .merkle
        .batch_archive
        .iter()
        .next()
        .map(|b| hex::encode(b.value().merkle_root))
        .unwrap_or_else(|| "0".repeat(64));

    // Delegate transformation to the dedicated domain builder
    let otlp_payload = otel::build_otlp_trace_from_timeline(
        &agent_hex,
        &state.config.tenant_id,
        &active_root,
        &nodes,
    );

    Ok(Json(otlp_payload))
}

// =================================================================
// ROUTER 1: AGENT DATA PLANE (TCP port 8081)
// =================================================================
// Exposes ONLY runtime execution, replay, and proof verification endpoints.

pub fn build_data_router(state: ApiState) -> axum::Router {
    let router = axum::Router::new()
        .route(
            "/health",
            get(|| async { Json(json!({"status": "ok", "version": "0.2.0"})) }),
        )
        .route("/v1/state/proof/{tx_id}", get(get_state_proof_handler))
        .route("/v1/effect/preflight", post(preflight_effect_handler))
        .route("/v1/effect/record", post(record_effect_handler))
        .route("/v1/effect/get", post(get_effect_handler))
        .route("/v1/swarm/ingress", post(http_ingress_endpoint))
        .route(
            "/v1/session/timeline/{agent_hex}/export/otel",
            get(export_agent_timeline_otel),
        )
        .layer(CatchPanicLayer::new())
        .with_state(state);

    router
}

// ==================================================================
// ROUTER 2: Operator Admin Plane (Unix Domain Socket `./vault/admin.sock`) [0600]
// ==================================================================
// Protectd by both OS filesystem `0600` permissions AND `ValidatedIdentity` Bearer token Verification.

pub fn build_admin_router(state: ApiState) -> axum::Router {
    axum::Router::new()
        .route(
            "/health",
            get(|| async { Json(json!({"status": "ok", "plane": "admin", "version": "0.2.0"})) }),
        )
        .route(
            "/v1/policy/quarantine_list",
            get(active_quarantine_endpoint),
        )
        .route(
            "/v1/policy/quarantine_list",
            get(active_quarantine_endpoint),
        )
        .route("/v1/admin/quarantine/lift", post(lift_quarantine_handler))
        .route("/v1/policy/metrics", get(policy_metrics_endpoint))
        .route("/v1/admin/in_doubt", get(in_doubt_intents_endpoint))
        .route("/v1/admin/redact/{tx_id}", post(redact_record_endpoint))
        .route("/v1/admin/timeline/{agent_hex}", get(fetch_agent_timeline))
        .route("/v1/admin/records/recent", get(recent_records_endpoint))
        .route("/v1/admin/cluster/info", get(cluster_info_endpoint))
        .route("/v1/admin/ingress/toggle", post(toggle_ingress_endpoint))
        .route(
            "/v1/admin/compactor/trigger",
            post(trigger_compaction_endpoint),
        )
        .layer(CatchPanicLayer::new())
        .with_state(state)
}

/// Pure standard-library constant-time byte comparison (Mitigates timing analysis)
fn constant_time_compare(a: &[u8], b: &[u8]) -> bool {
    if a.len() != b.len() {
        return false;
    }
    let mut diff = 0u8;
    for (x, y) in a.iter().zip(b.iter()) {
        diff |= x ^ y;
    }
    diff == 0
}
