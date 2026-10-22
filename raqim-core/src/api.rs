use axum::extract::{Path, Query};
use axum::http::header::{AUTHORIZATION, HOST, ORIGIN};
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
use ed25519_dalek::{Signer, SigningKey};
use serde_json::{json, Value};
use std::time::{SystemTime, UNIX_EPOCH};
use std::{eprintln, format, println};

use serde::{Deserialize, Serialize};
use std::result::Result::{Err, Ok};
use std::{collections::HashMap, sync::Arc};
use tokio::sync::broadcast::Sender;

use crate::cold_store::ColdStore;
use crate::compactor::WalCompactor;
use crate::merkle::{InclusionProof, MerkleEngine};
use crate::otel::{self, OtelExportRequest};
use crate::policy::{CapabilityCertificate, QuarantineRecord};
use crate::registry::SwarmRegistry;
use crate::wal::WalEngine;
use crate::{
    config::RaqimConfig,
    effect_store::{EffectStore, UnifiedSearchResult},
    policy::PolicyEngine,
};
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

    pub mem_router: Arc<EffectStore>,
    pub merkle: Arc<MerkleEngine>,
    pub policy: Arc<PolicyEngine>,
    pub wal: Arc<WalEngine>,
    pub cold_store: Arc<dyn ColdStore>,

    pub event_tx: Sender<SystemEvent>,
    pub swarm_registry: Arc<SwarmRegistry>,
    pub master_signing_key: SigningKey,

    pub pause_tx: Arc<watch::Sender<bool>>,

    pub compactor: Arc<WalCompactor>,
}

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

    async fn from_request_parts(parts: &mut Parts, _state: &S) -> Result<Self, Self::Rejection> {
        // BARRIER 1: Drive-By Browser Exploit Shield (Cross-Origin Protection)
        if let Some(origin) = parts.headers.get(ORIGIN).and_then(|v| v.to_str().ok()) {
            let allowed_origins = [
                "http://localhost:3000",
                "http://127.0.0.1:3000",
                "http://localhost:8081",
                "http://127.0.0.1:8081",
            ];

            let is_allowed = allowed_origins.iter().any(|&allowed| allowed == origin);
            if !is_allowed {
                eprintln!(
                    "[SECURITY ALERT] Blocked cross-origin drive-by attempt from Origin: {}",
                    origin
                );
                return Err(ApiError::Forbidden(format!(
                    "Cross-Origin Security Interdiction: Access from '{}' denied",
                    origin
                )));
            }
        }

        // BARRIER 2: Resolve Runtime Environment & Loopback Bypass
        let is_production = std::env::var("RAQIM_ENV")
            .map(|v| v.to_lowercase() == "production")
            .unwrap_or(false);

        // In local development, loopback requests from Next.js console are admitted
        if !is_production {
            if let Some(host) = parts.headers.get(HOST).and_then(|v| v.to_str().ok()) {
                if host.starts_with("localhost:") || host.starts_with("127.0.0.1:") {
                    return Ok(ValidatedIdentity {
                        tenant_id: "local_open_core".to_string(),
                        is_admin: true,
                    });
                }
            }
        }

        // BARRIER 3: Production / Cloud VPS / Docker Hardening
        let auth_header = match parts
            .headers
            .get(AUTHORIZATION)
            .and_then(|val| val.to_str().ok())
        {
            Some(hdr) => hdr,
            None => {
                return Err(ApiError::Unauthorized(
                    "Missing 'Authorization: Bearer <token>' header".to_string(),
                ));
            }
        };

        if !auth_header.starts_with("Bearer ") {
            return Err(ApiError::Unauthorized(
                "Invalid auth scheme. Use 'Bearer <token>'".to_string(),
            ));
        }

        let provided_token = auth_header.trim_start_matches("Bearer ").trim();

        let expected_secret = std::env::var("RAQIM_ADMIN_SECRET").unwrap_or_else(|_| {
            std::fs::read_to_string("keys/admin.secret")
                .unwrap_or_default()
                .trim()
                .to_string()
        });

        if expected_secret.is_empty() {
            return Err(ApiError::InternalServerError(
                "Security Gate Block: RAQIM_ADMIN_SECRET is empty on production daemon".to_string(),
            ));
        }

        // Constant-time byte comparison mitigates timing attacks
        if constant_time_compare(provided_token.as_bytes(), expected_secret.as_bytes()) {
            Ok(ValidatedIdentity {
                tenant_id: "production_tenant".to_string(),
                is_admin: true,
            })
        } else {
            Err(ApiError::Unauthorized(
                "Access Denied: Invalid Administrative Token".to_string(),
            ))
        }
    }
}

#[derive(Serialize, Clone, Debug)]
pub struct ActiveAgentNode {
    pub namespace: String,
    pub status: String, // Active, Quarantined, Idle
}

#[derive(Deserialize)]
pub struct RecentThoughtsParams {
    pub limit: Option<usize>,
}

pub async fn recent_record_endpoint(
    _auth: ValidatedIdentity,
    State(state): State<ApiState>,
    Query(params): Query<RecentThoughtsParams>,
) -> Result<Json<Vec<RecentRecord>>, ApiError> {
    let limit = params.limit.unwrap_or(50).min(500);

    // 1. Gather thoughts from active RAM buffers
    let ram_thoughts = state.merkle.get_recent_record_ram(limit);

    // 2. Gather thoughts from WAL disk
    let wal_thoughts = state
        .wal
        .read_recent_wal_thoughts(&state.config.wal_path, limit);

    // 3. Merge and deduplicate by tx_id
    let mut seen = std::collections::HashSet::new();
    let mut all_thoughts = Vec::new();

    for t in ram_thoughts {
        if seen.insert(t.tx_id.clone()) {
            all_thoughts.push(t);
        }
    }

    for t in wal_thoughts.into_iter().rev() {
        if seen.insert(t.tx_id.clone()) {
            all_thoughts.push(t);
        }
    }

    // Sort chronologically (oldest first so UI appends cleanly)
    all_thoughts.sort_by(|a, b| a.timestamp.cmp(&b.timestamp));
    if all_thoughts.len() > limit {
        all_thoughts = all_thoughts.split_off(all_thoughts.len() - limit);
    }

    Ok(Json(all_thoughts))
}

#[derive(Serialize, Clone, Debug)]
pub struct VaultSearchResult {
    pub tx_id: u128,
    pub tx_id_hex: String,
    pub agent_hex: String,
    pub namespace: String,
    pub payload: String,
    pub text: String,
    pub timestamp: String,
    pub source: String,
    pub similarity_score: f32,
    pub score: f32,
}

impl From<UnifiedSearchResult> for VaultSearchResult {
    fn from(res: UnifiedSearchResult) -> Self {
        Self {
            tx_id: res.tx_id,
            tx_id_hex: format!("{:032x}", res.tx_id),
            agent_hex: res.agent_hex,
            namespace: res.namespace,
            payload: res.text.clone(),
            text: res.text,
            timestamp: res.timestamp.to_string(),
            source: res.source.to_string(),
            similarity_score: res.score,
            score: res.score,
        }
    }
}

pub async fn active_qurantine_endpoint(
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
struct ResurrectPayload {
    agent_hex: String,
    system_prompt_override: String,
}

async fn lift_qurantine_and_resurrect(
    _auth: ValidatedIdentity,
    State(state): State<ApiState>,
    Json(payload): Json<ResurrectPayload>,
) -> Result<Json<Value>, ApiError> {
    // Fire the Out-of-Band Context Eviction Via Zenoh
    println!(
        "[AEGIS] Dispatching Context Eviction to: {}... ",
        payload.agent_hex.clone()
    );

    // Unfreeze the Agent (Remove from RAM)
    if state
        .policy
        .quarantine_blocklist
        .remove(&payload.agent_hex)
        .is_some()
    {
        // Append to durable control journal
        let _ = crate::checkpoint::CheckpointEngine::append_control_mutation(
            std::path::Path::new("./vault/control_journal.bin"),
            &crate::checkpoint::ControlMutation::LiftQuarantine {
                agent_hex: payload.agent_hex.clone(),
            },
        );

        // Also update the Ram process table so the Topology page knows it's alive again.
        // TODO: Update the  namespace
        state
            .swarm_registry
            .touch_agent(&payload.agent_hex, "Unknown", "Rebooting", "Unknown");

        println!(
            "[AEGIS] Agent {} quarantine lifted. Reality re-seeded.",
            payload.agent_hex
        );

        match state
            .mem_router
            .boot_historical_agent(&payload.agent_hex, None, None, false)
            .await
        {
            Ok(()) => Ok(Json(
                json!({"success": true, "message": "Quarantine lifted"}),
            )),

            Err(e) => {
                let err_str = e.to_string();
                if err_str.contains("no immutable memory on disk") {
                    println!(
                        "[TIME MACHINE] Agent {} has no prior WAL history; active at Genesis state.", payload.agent_hex
                    );

                    Ok(Json(json!({
                        "success": true,
                        "message": "Quarantine lifted (Genesis state; zero historical commits)"
                    })))
                } else {
                    eprintln!(
                        "[TIME MACHINE FATAL] Failed to resurrect WASM state for {}: {} ",
                        &payload.agent_hex, e
                    );
                    Err(ApiError::InternalServerError(format!(
                        "Resurrection failed: {}",
                        e
                    )))
                }
            }
        }
    } else {
        Err(ApiError::NotFound(format!(
            "Agent {} not in quarantine",
            payload.agent_hex
        )))
    }
}

#[derive(Deserialize, Clone)]
pub struct ForkConfig {
    pub override_seed: Option<u64>,
    pub inject_network: Option<String>,
    pub env_overrides: HashMap<String, String>,
    pub config_overrides: HashMap<String, String>,
}

#[derive(Deserialize)]
struct TimeTravelRequest {
    agent_hex: String,
    target_tx_id: u128,
    fork_config: ForkConfig,
}

// THE ACTIVE DEBUGGING ROUTE HANDLER
async fn time_travel_endpoint(
    _identity: ValidatedIdentity,
    State(state): State<ApiState>,
    Json(payload): Json<TimeTravelRequest>,
) -> Result<Json<Value>, ApiError> {
    println!(
        "[TIME TRAVEL] Admin requested Reality Forkk for Agent {} at TxID {} ",
        payload.agent_hex, payload.target_tx_id
    );

    // 1. Lift policy Quarantine so that the agent can actually boot
    // Unfreeze the Agent (Remove from DashhMap)
    if state
        .policy
        .quarantine_blocklist
        .remove(&payload.agent_hex)
        .is_some()
    {
        match state
            .mem_router
            .boot_historical_agent(
                &payload.agent_hex,
                Some(payload.target_tx_id),
                Some(payload.fork_config),
                true,
            )
            .await
        {
            Ok(()) => Ok(Json(
                json!({"success": true, "message": "Time travel initiated" }),
            )),

            Err(e) => Err(ApiError::InternalServerError(format!(
                "Time travel failed: {}",
                e
            ))),
        }
    } else {
        Err(ApiError::NotFound(format!(
            "Agent hex: {} not found in quarantine blocklist",
            &payload.agent_hex
        )))
    }
}

// THE ZERO-COPY HTTP INGRESS: The endpoint expects raw binary `rkyv` bytes, Not JSON.
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

    let path_intent = archived_ingress.intent_path.as_str();

    let agent_pub_key: [u8; 32] = archived_ingress
        .public_key
        .try_into()
        .map_err(|_| ApiError::BadRequest("Public key must be exactly 32 bytes".to_string()))?;

    // O(1) Aegis Policy Check.
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
            &path_intent,
            packet_ts_sec,
        )
        .map_err(|e| ApiError::Forbidden(format!("Aegis Interdiction: {}", e)))?;

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
    let lance_future = state.cold_store.fetch_historical_timeline(&agent_hex);
    let wal_future = async {
        state
            .wal
            .fetch_hot_timeline(&agent_hex, &state.config.wal_path)
    };

    let (lance_res, wal_res) = tokio::join!(lance_future, wal_future);

    // The Gather
    let mut nodes = wal_res.unwrap_or_default();
    if let Ok(mut cold_res) = lance_res {
        nodes.append(&mut cold_res)
    }

    nodes.sort_by(|a, b| a.tx_id.cmp(&b.tx_id));

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
        if new_state {
            "PAUSED (TCP ZERO-WINDOW) "
        } else {
            "ACTIVE"
        }
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
    pub remaining_tokens: u64,
}

#[derive(Serialize)]
pub struct AegisMetricsData {
    pub total_quarantined: usize,
    pub recent_interdictions: usize,
    pub signarure_spoofs: usize,
    pub namespace_breaches: usize,
    pub rate_limit_blocks: usize,
    pub active_policies: Vec<GroupPolicyTelemetry>,
}

pub async fn aegis_metics_endpoint(
    _auth: ValidatedIdentity,
    State(state): State<ApiState>,
) -> Result<Json<AegisMetricsData>, ApiError> {
    let mut metrics = AegisMetricsData {
        total_quarantined: 0,
        recent_interdictions: 0,
        signarure_spoofs: 0,
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
            "CRYPTO_SPOOF" => metrics.signarure_spoofs += 1,
            "NAMESPACE_BREACH" => metrics.namespace_breaches += 1,
            "RATE_LIMIT_EXCEEDED" => metrics.rate_limit_blocks += 1,
            _ => {}
        }
    }

    // Query live Atomic token bucket across active policies
    let policies_guard = state.policy.group_policies.read();
    for (group_name, policy) in policies_guard.iter() {
        let current_tokens = policy
            .rate_limiter
            .tokens
            .load(std::sync::atomic::Ordering::Relaxed);
        metrics.active_policies.push(GroupPolicyTelemetry {
            group_name: group_name.clone(),
            allowed_namspace: policy.allowed_namespaces.clone(),
            blocked_namespace: policy.blocked_namespaces.clone(),
            max_tps: policy.rate_limiter.max_tps,
            burst_capacity: policy.rate_limiter.burst_capacity,
            remaining_tokens: current_tokens,
        });
    }

    Ok(Json(metrics))
}

#[derive(serde::Deserialize)]
pub struct MintRequest {
    pub agent_hex: String,
    pub group: String,
}

pub async fn handle_ca_mint(
    _auth: ValidatedIdentity,
    State(state): State<ApiState>,
    Json(payload): Json<MintRequest>,
) -> Result<Json<String>, ApiError> {
    // Contruct the unsigned Certificate Passport
    let expiration = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs()
        + (365 * 24 * 60 * 60);

    let mut cert = CapabilityCertificate {
        agent_hex: payload.agent_hex.clone(),
        group_name: payload.group.clone(),
        expiration_timestamp: expiration,
        master_signature: Vec::new(),
    };

    // Serialize and sign using the master private key inside api_state
    let serialized_raw = postcard::to_allocvec(&cert)
        .map_err(|e| ApiError::InternalServerError(format!("Serialization failed: {}", e)))?;

    let signature = state.master_signing_key.sign(&serialized_raw);

    cert.master_signature = signature.to_bytes().to_vec();

    // Returned the fully serialized and signed passport to the CLI
    let final_bytes = postcard::to_allocvec(&cert)
        .map_err(|e| ApiError::InternalServerError(format!("Final packing failed: {}", e)))?;

    Ok(Json(hex::encode(final_bytes)))
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

    let payload = json!({
        "highest_tx_id": format!("0x{:032x}", highest_tx),
        "wal_bytes": wal_size,
        "wal_size_mb": (wal_size as f64 ) / (1024.0 * 1024.0),
        "buffer_load": pending_wal_items,
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
}

/// Executes cryptographic zero-trust pre-flight audit before any tool or effect executes.
pub async fn preflight_effect_handler(
    State(state): State<ApiState>,
    Json(payload): Json<PreflightEffectRequest>,
) -> Result<Json<PreflightEffectResponse>, ApiError> {
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

    // Aegis barrier 1: Lineage Verification
    let (verified_agent_hex, group_name) = state
        .policy
        .verify_session_lineage(&capability_cert, &pub_key)
        .map_err(|e| ApiError::Unauthorized(format!("Aegis lineage rejection: {}", e)))?;

    if verified_agent_hex != payload.agent_hex {
        return Err(ApiError::Forbidden(
            "Security violation: Certificate identity mismatch with caller".to_string(),
        ));
    }

    if state.policy.is_quarantined(&payload.agent_hex) {
        return Err(ApiError::Forbidden(
            "Agent is actively quarantined by Aegis firewall".to_string(),
        ));
    }

    let canonical_attestation = format!(
        "preflight:{}:{}:{}:{}",
        payload.namespace, payload.step_ordinal, payload.call_signature_hex, payload.timestamp
    );

    // Aegis Barrier 2: Fast-path policy audit (anti-replay, signature, rate-limiting, and namespace ACL)
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
        .map_err(|e| ApiError::Forbidden(format!("Aegis Interdiction: {}", e)))?;

    Ok(Json(PreflightEffectResponse {
        allowed: true,
        agent_hex: payload.agent_hex,
        namespace: payload.namespace,
    }))
}

/// Records live side-effect into WAL + Markle DAG
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

    // Aegis barrier 1: Lineage Verification
    let (verified_agent_hex, group_name) = state
        .policy
        .verify_session_lineage(&capability_cert, &pub_key)
        .map_err(|e| ApiError::Unauthorized(format!("Aegis lineage rejection: {}", e)))?;

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

    // Aegis Barrier 2: Fast-path policy audit
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
        .map_err(|e| ApiError::Forbidden(format!("Aegis Interdiction: {}", e)))?;

    let is_forked = payload.namespace.starts_with("phantom_");

    match state
        .mem_router
        .record_effect(
            agent_id,
            payload.step_ordinal,
            call_signature_hash,
            output_payload,
            &payload.namespace,
        )
        .await
    {
        Ok(tx_id) => {
            let original_ns = payload.namespace.clone().replace("phantom_", "");

            if is_forked {
                let tx_id = format!("{:032x}", tx_id);
                let timestamp = std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .unwrap_or_default()
                    .as_secs() as i64;

                // Audit Trail: Emitted to System Event Bus -> Persisted to lanceDB System event table
                let _ = state.event_tx.send(SystemEvent::RealityForked {
                    agent_id: payload.agent_hex.clone(),
                    original_namespace: original_ns.clone(),
                    phantom_namespace: payload.namespace.clone(),
                    step_ordinal: payload.step_ordinal,
                    tx_id: tx_id.clone(),
                    timestamp,
                });
            }

            Ok(Json(RecordEffectResponse {
                success: true,
                tx_id_hex: format!("{:032x}", tx_id),
                is_forked_branch: is_forked,
            }))
        }

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
        .mem_router
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

    // Query Axon Gatekeeper for 0(log N) Inclusion proof
    match state.merkle.generate_proof_for_tx(tx_id) {
        Some(proof) => Ok(Json(StateProofResponse {
            success: true,
            proof: Some(proof.clone()),
            message: if proof.is_active_buffer {
                "Proof generated against active workspace buffer (un-crystallines)".to_string()
            } else {
                "proof generated against immutable Markle Batch archive.".to_string()
            },
        })),

        None => Ok(Json(StateProofResponse {
            success: false,
            proof: None,
            message: format!(
                "Transaction ID {:032x} not found in active memory batch archives.",
                tx_id
            ),
        })),
    }
}

pub async fn trigger_compaction_endpoint(
    State(state): State<ApiState>,
) -> Result<Json<serde_json::Value>, ApiError> {
    println!("[ADMIN] Manual on-demand WAL compaction requested via HTTP API... ");

    let compactor = state.compactor.clone();

    tokio::spawn(async move {
        match compactor.trigger_safe_compaction().await {
            Ok(count) => {
                println!(
                    "[ADMIN SUCCESS] Background compaction complete. Archived {} thoughts to LanceDB.",
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
        "message": "WAL rotation and 2PC LanceDB assimilation initiated in background worker."
    })))
}

pub async fn export_agent_timeline_otel(
    _auth: ValidatedIdentity,
    State(state): State<ApiState>,
    Path(agent_hex): Path<String>,
) -> Result<Json<OtelExportRequest>, ApiError> {
    // Scatter-gather historical timeline from lanceDB and WAL
    let lance_future = state.cold_store.fetch_historical_timeline(&agent_hex);
    let wal_future = async {
        state
            .wal
            .fetch_hot_timeline(&agent_hex, &state.config.wal_path)
    };

    let (lance_res, wal_res) = tokio::join!(lance_future, wal_future);

    let mut nodes = wal_res.unwrap_or_default();
    if let Ok(mut cold_res) = lance_res {
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
        .map(|b| hex::encode(b.value().markle_root))
        .unwrap_or_else(|| {
            "0000000000000000000000000000000000000000000000000000000000000000".to_string()
        });
    // -- what happens if the agent never has a merkle root? Maybe  the buffer is still active and yeet to seal?

    // Delegate transformation to the dedicated domain builder
    let otlp_payload = otel::build_otlp_trace_from_timeline(
        &agent_hex,
        &state.config.tenant_id,
        &active_root,
        &nodes,
    );

    Ok(Json(otlp_payload))
}

// Route Builder
pub fn build_admin_router(state: ApiState) -> axum::Router {
    axum::Router::new()
        // Health check endpoint
        .route(
            "/health",
            get(|| async { axum::Json(serde_json::json!({"status": "ok", "version": "0.1.2"})) }),
        )
        // State Proofs & Effect Recording
        .route("/v1/state/proof/:tx_id", get(get_state_proof_handler))
        .route("/v1/effect/preflight", post(preflight_effect_handler))
        .route("/v1/effect/record", post(record_effect_handler))
        .route("/v1/effect/get", post(get_effect_handler))
        // Aegis Firewall & Governance
        .route("/v1/policy/quarantine_list", get(active_qurantine_endpoint))
        .route(
            "/v1/admin/quarantine/lift",
            post(lift_qurantine_and_resurrect),
        )
        .route("/v1/policy/metrics", get(aegis_metics_endpoint))
        .route("/v1/admin/ca/mint", post(handle_ca_mint))
        // Time Machine & Reality Forking
        .route("/v1/admin/time_travel", post(time_travel_endpoint))
        .route("/v1/time_travel/fork", post(time_travel_endpoint))
        .route("/v1/admin/time_travel/fork", post(time_travel_endpoint))
        .route(
            "/v1/admin/time_travel/timeline/:agent_hex",
            get(fetch_agent_timeline),
        )
        // Cluster Observervability and Diagnostics
        .route("/v1/admin/cluster/info", get(cluster_info_endpoint))
        .route("/v1/admin/ingress/toggle", post(toggle_ingress_endpoint))
        .route(
            "/v1/admin/compactor/trigger",
            post(trigger_compaction_endpoint),
        )
        // System & Agent Deployment endpoints
        .route("/v1/system/thoughts/recent", get(recent_record_endpoint))
        // Swarm & A2A Ingress
        .route("/v1/swarm/ingress", post(http_ingress_endpoint))
        .route(
            "/v1/session/timeline/:agent_hex/export/otel",
            get(export_agent_timeline_otel),
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
