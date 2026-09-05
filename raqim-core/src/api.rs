use axum::extract::ws::{Message, WebSocket, WebSocketUpgrade};
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
use axum::response::sse::{Event, KeepAlive, Sse};
use dashmap::DashMap;
use ed25519_dalek::{Signer, SigningKey};
use futures_util::stream::Stream;
use futures_util::{stream::StreamExt, SinkExt};
use serde_json::{json, Value};
use std::convert::Infallible;
use std::time::{SystemTime, UNIX_EPOCH};
use std::{eprintln, format, println};
use tokio_stream::wrappers::BroadcastStream;

use serde::{Deserialize, Serialize};
use std::result::Result::{Err, Ok};
use std::{collections::HashMap, sync::Arc};
use tokio::sync::broadcast::Sender;
use tokio::sync::{mpsc, oneshot};
use tokio::time::{timeout, Duration};
use uuid::Uuid;

use crate::aegis::{CapabilityCertificate, QuarantineRecord};
use crate::axon::{AxonGateKeeper, InclusionProof};
use crate::compactor::WalCompactor;
use crate::health::SystemHealth;
use crate::hot_memory::HotVectorBuffer;
use crate::lancedb_store::LanceEngine;
use crate::nucleus::WalEngine;
use crate::registry::SwarmRegistry;
use crate::state::SwarmStateRegistry;
use crate::{
    aegis::AegisGateKeeper, config::RaqimConfig, memory_router::MemoryRouter,
    network::GlobalNetworkBridge, A2AEnvelope,
};
use crate::{execute_raqim_cascade, AgentState, IngressEnvelope, SystemEvent};

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

// Websocket Message Types & UI Event Schemas
#[derive(Serialize, Deserialize, Debug)]
#[serde(tag = "type")] // Enables json parsing {"type": "AskQuestion", }
pub enum WsMessage {
    // Python -> Daemon: "I want to listen here"
    RegisterCapability {
        capability: String,
    },

    // Python -> Daemon: "Ask the swarm this question"
    AskQuestion {
        request_id: String,
        capability: String,
        question: Vec<u8>,
        sender_hex: String,
        public_key: String,
        signature: Vec<u8>,
        capability_cert: String,
    },

    // Daemon -> python: "Someone is asking you a question"
    IncomingQuestion {
        request_id: String,
        capability: String,
        question: Vec<u8>,
    },

    // Python -> Daemom: "Here's my answer to the incoming question"
    ReplyToQuestion {
        request_id: String,
        answer: Vec<u8>,
        responder_hex: String,
    },

    // Deamon -> Python: "Here's the answer for the AskQueustion you sent earlier"
    QuestionAnswered {
        request_id: String,
        answer: Vec<u8>,
    },

    Error {
        message: String,
    },
}

#[derive(serde::Serialize, Clone, Debug)]
#[serde(tag = "event_type")]
pub enum UiEvent {
    ThoughtCommitted {
        agent_hex: String,
        intent_path: String,
        tx_id: String,
        text: String,
    },

    RealityForked {
        agent_id: String,
        original_namespace: String,
        phantom_namespace: String,
        step_ordinal: u64,
        tx_id: String,
    },

    A2aMessageRouted {
        source_hex: String,
        target_hex: String,
        namespace: String,
        question_payload: String,
        answer_payload: String,
        latency_ms: u32,
    },

    AegisAlert {
        record: QuarantineRecord,
    },
}

#[derive(Clone)]
pub struct ApiState {
    pub config: Arc<RaqimConfig>,

    pub mem_router: Arc<MemoryRouter>,
    pub axon: Arc<AxonGateKeeper>,
    pub brain: Arc<SwarmStateRegistry>,
    pub aegis: Arc<AegisGateKeeper>,
    pub global_net: Arc<GlobalNetworkBridge>,
    pub wal: Arc<WalEngine>,
    pub lance: Arc<LanceEngine>,

    pub event_tx: Sender<SystemEvent>,
    pub ui_tx: Sender<UiEvent>,
    pub phantom_ui_tx: Sender<UiEvent>,
    pub health_tx: Sender<SystemHealth>,
    pub swarm_registry: Arc<SwarmRegistry>,
    pub master_signing_key: SigningKey,

    pub hot_buffer: Arc<HotVectorBuffer>,
    pub pause_tx: Arc<watch::Sender<bool>>,

    pub compactor: Arc<WalCompactor>,
}

#[derive(Clone, Debug)]
pub struct ValidatedIdentity {
    pub tenant_id: String,
    pub is_admin: bool,
}

#[axum::async_trait]
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

// The shared state for this speciific ws connection
struct WsConnectionstate {
    // Maps req_id -> the pipe that wakes up the waiting zenoh thread
    pending_a2a_requests: DashMap<String, oneshot::Sender<(Vec<u8>, String)>>,
    // Channel to send mesages DOWN to the Python client
    downstream_tx: mpsc::Sender<Message>,
}

// 3. The Axum Handler (Protected by ValidatedIdentity)
pub async fn mcp_ws_handler(
    _auth: ValidatedIdentity,
    State(state): State<ApiState>,
    ws: WebSocketUpgrade,
) -> Response {
    ws.on_upgrade(move |socket| handle_mcp_socket(socket, state))
}

pub async fn handle_mcp_socket(socket: WebSocket, state: ApiState) {
    let (mut ws_sender, mut ws_receiver) = socket.split();
    let (downstream_tx, mut downstream_rx) = mpsc::channel::<Message>(100);

    let conn_state = Arc::new(WsConnectionstate {
        pending_a2a_requests: DashMap::new(),
        downstream_tx: downstream_tx.clone(),
    });

    // Task 1: Forward downstream message to the actual WS
    let mut send_task = tokio::spawn(async move {
        while let Some(msg) = downstream_rx.recv().await {
            if ws_sender.send(msg).await.is_err() {
                break;
            }
        }
    });

    // Task 2: Process incoming message from Python
    let conn_state_clone = conn_state.clone();
    let mut recv_task = tokio::spawn(async move {
        while let Some(Ok(msg)) = ws_receiver.next().await {
            if let Message::Text(text) = msg {
                if let Ok(ws_msg) = serde_json::from_str::<WsMessage>(&text) {
                    process_ws_message(ws_msg, conn_state_clone.clone(), state.clone()).await;
                }
            }
        }
    });

    // If either task fails (socket closed), kill both.
    tokio::select! {
        _ = (&mut send_task) => recv_task.abort(),
        _ = (&mut recv_task) => send_task.abort()
    };
}

// The Memory router
async fn process_ws_message(msg: WsMessage, conn: Arc<WsConnectionstate>, os_state: ApiState) {
    match msg {
        WsMessage::RegisterCapability { capability } => {
            let conn_clone = conn.clone();
            let cap_clone = capability.clone();

            // OS spawns the zenoh listener.
            tokio::spawn(async move {
                os_state
                    .global_net
                    .register_agent_capability(&capability, move |question_bytes| {
                        let request_id = Uuid::new_v4().to_string();
                        let (reply_tx, reply_rx) = oneshot::channel();

                        // Register pending response channel
                        conn_clone
                            .pending_a2a_requests
                            .insert(request_id.clone(), reply_tx);

                        let incoming_msg = WsMessage::IncomingQuestion {
                            request_id: request_id.clone(),
                            capability: cap_clone.clone(),
                            question: question_bytes.to_vec(),
                        };

                        // Send down to python
                        if let Ok(json_str) = serde_json::to_string(&incoming_msg) {
                            let tx = conn_clone.downstream_tx.clone();
                            tokio::spawn(async move {
                                let _ = tx.send(Message::Text(json_str)).await;
                            });
                        }

                        // ZERO CPU WAIT: Suspends thread qith 15-sec timeout waiting for python reply
                        match tokio::runtime::Handle::current()
                            .block_on(timeout(Duration::from_secs(15), reply_rx))
                        {
                            Ok(Ok((answer, responder_hex))) => {
                                // Format structured JSON reply payload for Zenoh query return
                                let reply_payload = serde_json::json!({
                                    "responder_hex": responder_hex,
                                    "answer": answer
                                });

                                serde_json::to_vec(&reply_payload).unwrap_or(answer)
                            }
                            _ => {
                                // Python crashed or too long. Clean up the DashMap to prevent memory leaks.
                                conn_clone.pending_a2a_requests.remove(&request_id);
                                b"A2A_TIMEOUT_OR_CRASH".to_vec()
                            }
                        }
                    })
                    .await;
            });
        }

        WsMessage::ReplyToQuestion {
            request_id,
            answer,
            responder_hex,
        } => {
            // Remove the wakeup pipe from dashmap and fire the answer into it!
            if let Some((_, reply_tx)) = conn.pending_a2a_requests.remove(&request_id) {
                let _ = reply_tx.send((answer, responder_hex));
            }
        }

        WsMessage::AskQuestion {
            request_id,
            capability,
            question,
            sender_hex,
            public_key,
            signature,
            capability_cert,
        } => {
            let os_state_clone = os_state.clone();
            let conn_clone = conn.clone();

            tokio::spawn(async move {
                // Decode Raw bytes from Hex container
                let cert_bytes = match hex::decode(&capability_cert) {
                    Ok(b) => b,
                    Err(_) => return,
                };

                let mut public_key_bytes = [0u8; 32];
                if let Ok(b) = hex::decode(&public_key) {
                    if b.len() == 32 {
                        public_key_bytes.copy_from_slice(&b);
                    }
                }

                let mut sender_id_bytes = [0u8; 16];
                if let Ok(decoded) = hex::decode(&sender_hex) {
                    if decoded.len() == 16 {
                        sender_id_bytes.copy_from_slice(&decoded);
                    }
                }

                let mut sig_bytes = [0u8; 64];
                if signature.len() == 64 {
                    sig_bytes.copy_from_slice(&signature)
                }

                // Verify Aegis Lineage Certificate
                let (agent_hex, group_name) = match os_state_clone
                    .aegis
                    .verify_session_lineage(&cert_bytes, &public_key_bytes)
                {
                    Ok((agent, group)) => (agent, group),
                    Err(e) => {
                        let err = WsMessage::Error {
                            message: format!("[AEGIS LINEAGE FAILURE]: {}", e),
                        };

                        if let Ok(json_str) = serde_json::to_string(&err) {
                            let _ = conn_clone.downstream_tx.send(Message::Text(json_str)).await;
                        }

                        return;
                    }
                };

                let current_ts = SystemTime::now()
                    .duration_since(UNIX_EPOCH)
                    .unwrap_or_default()
                    .as_secs() as i64;

                if let Err(e) = os_state_clone.aegis.authorize_packet_fast(
                    agent_hex.as_str(),
                    group_name.as_str(),
                    &public_key_bytes,
                    &question,
                    &sig_bytes,
                    &capability,
                    current_ts,
                ) {
                    let err = WsMessage::Error {
                        message: format!("[AEGIS Gate block] {}  ", e),
                    };

                    if let Ok(json_str) = serde_json::to_string(&err) {
                        let _ = conn_clone.downstream_tx.send(Message::Text(json_str)).await;
                    }

                    return;
                };

                // Seal outgoing question (Tx_ask) into WAL + Merkle DAG
                let ask_tx_id = uuid::Uuid::now_v7().as_u128();
                let ask_state = AgentState {
                    agent_id: Some(sender_id_bytes),
                    transaction_id: ask_tx_id,
                    namespace: format!("/swarm/a2a/ask/{}", capability),
                    timestamp: current_ts,
                    status: crate::AgentStatus::ToolExecution,
                    text: String::from_utf8_lossy(&question).to_string(),
                };

                let mut aligned_ask_buf = rkyv::util::AlignedVec::<16>::new();
                if let Ok(sb) = rkyv::to_bytes::<rkyv::rancor::Error>(&ask_state) {
                    aligned_ask_buf.extend_from_slice(&sb);
                }

                // Compute deterministic leaf hash of the questionfor causal chaining
                let mut ask_hasher = blake3::Hasher::new_derive_key("raqim.axon.v1.leaf");
                ask_hasher.update(&aligned_ask_buf);
                ask_hasher.update(&sender_id_bytes);
                let ask_leaf_hash: [u8; 32] = ask_hasher.finalize().into();

                if let Ok(archived_ask_state) = rkyv::access::<
                    <AgentState as rkyv::Archive>::Archived,
                    rkyv::rancor::Error,
                >(&aligned_ask_buf)
                {
                    let _ = execute_raqim_cascade(
                        archived_ask_state,
                        os_state_clone.axon.clone(),
                        os_state_clone.wal.clone(),
                        os_state_clone.brain.clone(),
                        os_state_clone.global_net.clone(),
                        os_state_clone.event_tx.clone(),
                        Vec::new(),
                        Vec::new(),
                    )
                    .await;
                }

                let envelope = A2AEnvelope {
                    sender_id: sender_id_bytes,
                    sender_public_key: public_key_bytes,
                    target_capability: capability.clone(),
                    payload: question.clone(),

                    signature: sig_bytes,
                    sender_capability_cert: cert_bytes,
                    timestamp: current_ts,
                };

                // Start the stopwatch
                let start_time = std::time::Instant::now();

                // Dispatch verified RPC across Zenoh Mesh
                match os_state_clone
                    .global_net
                    .execute_a2a_rpc(envelope, os_state_clone.aegis.clone())
                    .await
                {
                    Ok((answer, responder_hex)) => {
                        // stop the stopwatch
                        let latency_ms = start_time.elapsed().as_millis() as u32;

                        // Seal verified Answer (Tx_reply) into WAL + Merkle DAG
                        let reply_tx_id = uuid::Uuid::now_v7().as_u128();
                        let reply_ts = SystemTime::now()
                            .duration_since(UNIX_EPOCH)
                            .unwrap_or_default()
                            .as_secs() as i64;

                        //Embed ask_tx_id and parent leaf hash into oreply payload
                        let anchored_reply_payload = serde_json::json!({
                         "causal_parent_tx": format!("{:032x}", ask_tx_id),
                         "causal_parent_hash": hex::encode(ask_leaf_hash),
                         "responder_hex": responder_hex,
                         "answer": String::from_utf8_lossy(&answer)
                        });

                        let reply_state = AgentState {
                            agent_id: Some(sender_id_bytes),
                            transaction_id: reply_tx_id,
                            timestamp: reply_ts,
                            status: crate::AgentStatus::Reasoning,
                            text: anchored_reply_payload.to_string(),
                            namespace: format!("/swarm/a2a/reply/{}", capability),
                        };

                        let mut aligned_reply_buf = rkyv::util::AlignedVec::<16>::new();
                        if let Ok(sb) = rkyv::to_bytes::<rkyv::rancor::Error>(&reply_state) {
                            aligned_reply_buf.extend_from_slice(&sb);
                        }

                        if let Ok(archived_reply_state) =
                            rkyv::access::<
                                <AgentState as rkyv::Archive>::Archived,
                                rkyv::rancor::Error,
                            >(&aligned_reply_buf)
                        {
                            let _ = execute_raqim_cascade(
                                archived_reply_state,
                                os_state_clone.axon.clone(),
                                os_state_clone.wal.clone(),
                                os_state_clone.brain.clone(),
                                os_state_clone.global_net.clone(),
                                os_state_clone.event_tx.clone(),
                                Vec::new(),
                                Vec::new(),
                            )
                            .await;
                        }

                        // Send the answer back to the waiting python sdk coroutine
                        let res = WsMessage::QuestionAnswered {
                            request_id,
                            answer: answer.clone(),
                        };

                        if let Ok(json_str) = serde_json::to_string(&res) {
                            let _ = conn_clone.downstream_tx.send(Message::Text(json_str)).await;
                        }

                        // Fire the laser beam to the UI
                        let ui_event = UiEvent::A2aMessageRouted {
                            source_hex: sender_hex,
                            target_hex: responder_hex,
                            namespace: capability.clone(),
                            question_payload: String::from_utf8_lossy(&question).into_owned(),
                            answer_payload: String::from_utf8_lossy(&answer).into_owned(),
                            latency_ms,
                        };

                        let _ = os_state_clone.ui_tx.send(ui_event);
                    }

                    Err(e) => {
                        let err = WsMessage::Error {
                            message: format!("[A2A Error] {}", e),
                        };

                        if let Ok(json_str) = serde_json::to_string(&err) {
                            let _ = conn_clone.downstream_tx.send(Message::Text(json_str)).await;
                        }
                    }
                }
            });
        }

        _ => {}
    }
}

#[derive(Serialize, Clone, Debug)]
pub struct ActiveAgentNode {
    pub namespace: String,
    pub status: String, // Active, Quarantined, Idle
}

#[derive(Serialize, Clone)]
pub struct UiThought {
    pub agent_hex: String,
    pub intent_path: String,
    pub text: String,
    pub tx_id: u64,
}

// 5. Rest & SSE route handlers.

// The Firehose Route Handler
pub async fn sse_firehose_endpoint(
    _auth: ValidatedIdentity,
    State(state): State<ApiState>,
) -> Sse<impl Stream<Item = Result<Event, Infallible>>> {
    // Subscribe to the live broadcast channel
    let receiver = state.ui_tx.subscribe();

    // Convert the Tokio Receiver into a standard async Stream.
    let stream = BroadcastStream::new(receiver).filter_map(|msg| async move {
        match msg {
            Ok(ui_event) => serde_json::to_string(&ui_event)
                .ok()
                .map(|json| Ok(Event::default().data(json))),

            Err(_) => {
                // Lagging subscribers are skipped automatically by tokio broadcast
                None
            }
        }
    });

    // Return the SSE stream to the browser.
    Sse::new(stream).keep_alive(KeepAlive::new())
}

// The Observatiton deck ( Only used by the time machine UI )
pub async fn sse_phantom_endpoint(
    _auth: ValidatedIdentity,
    State(state): State<ApiState>,
) -> Sse<impl Stream<Item = Result<Event, Infallible>>> {
    let receiver = state.phantom_ui_tx.subscribe();

    let stream = BroadcastStream::new(receiver).filter_map(|msg| async move {
        match msg {
            Ok(p_event) => serde_json::to_string(&p_event)
                .ok()
                .map(|json| Ok(Event::default().data(json))),

            Err(_) => None,
        }
    });

    Sse::new(stream).keep_alive(KeepAlive::new())
}

pub async fn sse_health_endpoint(
    _auth: crate::api::ValidatedIdentity,
    State(state): State<ApiState>,
) -> Sse<impl futures_util::Stream<Item = Result<Event, std::convert::Infallible>>> {
    let receiver = state.health_tx.subscribe();

    let stream = BroadcastStream::new(receiver).filter_map(|msg| async move {
        match msg {
            Ok(health_payload) => serde_json::to_string(&health_payload)
                .ok()
                .map(|json| Ok(Event::default().data(json))),

            Err(_) => None,
        }
    });

    Sse::new(stream).keep_alive(KeepAlive::new())
}

pub async fn agent_alias_endpoint(
    _auth: ValidatedIdentity,
    State(state): State<ApiState>,
) -> axum::Json<HashMap<String, String>> {
    let mut map = HashMap::new();
    for entry in state.swarm_registry.active_agents.iter() {
        map.insert(entry.key().clone().to_string(), entry.value().alias.clone());
    }
    axum::Json(map)
}

#[derive(Deserialize)]
pub struct UnifiedSearchQuery {
    pub query: String,
    pub namespace: Option<String>,
    pub include_wal: Option<bool>,
}

pub async fn unified_vault_search(
    _auth: ValidatedIdentity,
    State(state): State<ApiState>,
    Query(params): Query<UnifiedSearchQuery>,
) -> Result<Json<Vec<VaultSearchResult>>, ApiError> {
    // The Scatter: Launch both searches concurrently on different OS threads
    let lance_future = state
        .lance
        .semantic_search(&params.query, params.namespace.as_deref(), 50);

    let wal_future = async {
        // Only hit the disk if the user explicitely requested the WAL inclusion
        if params.include_wal.unwrap_or(true) {
            state.wal.lexical_scan(
                &params.query,
                params.namespace.as_deref(),
                50,
                &state.config.wal_path,
            )
        } else {
            Ok(vec![])
        }
    };

    let (lance_res, wal_res) = tokio::join!(lance_future, wal_future);

    // THE GATHER: Starting with the hot wal reasult
    let mut unified_results = wal_res.unwrap_or_default();

    if let Ok(mut cold_results) = lance_res {
        unified_results.append(&mut cold_results)
    }

    // Sort the unified results purely semantic score (Highest first)
    unified_results.sort_by(|a, b| {
        b.similarity_score
            .partial_cmp(&a.similarity_score)
            .unwrap_or(std::cmp::Ordering::Equal)
    });

    // Cap at top 100 for UI performance
    unified_results.truncate(100);

    Ok(Json(unified_results))
}

// 2. THE RAG SEMANTIC SEARCH ENDPOINT
#[derive(Deserialize)]
pub struct RagQuery {
    namespace: String,
    query: String,
    limit: Option<usize>,
}

pub async fn semantic_search_endpoint(
    _auth: ValidatedIdentity,
    State(state): State<ApiState>,
    Query(params): Query<RagQuery>,
) -> Result<Json<Vec<String>>, ApiError> {
    let limit = params.limit.unwrap_or(5);

    match state
        .mem_router
        .query_hybrid_memory(
            &params.query,
            Some(&params.namespace),
            limit,
            &state.hot_buffer,
        )
        .await
    {
        Ok(memories) => Ok(Json(memories)),
        Err(e) => {
            eprintln!("[RAG Hybrid ERROR] {}", e);
            Err(ApiError::InternalServerError(
                format!("RAG search failed: {}", e).to_string(),
            ))
        }
    }
}

#[derive(Serialize, Clone, Debug)]
pub struct VaultSearchResult {
    pub tx_id: u128,
    pub agent_hex: String,
    pub namespace: String,
    pub payload: String,
    pub timestamp: String,
    pub source: String,
    pub similarity_score: f32,
}

#[derive(Serialize, Clone, Debug)]
pub struct VaultTelemetry {
    pub total_vectors: usize,
    pub index_size_mb: f64,
    pub wal_pending_count: u64,
    pub densest_namespace: String,
    pub embedder_name: String,
    pub embeder_dim: usize,
}

pub async fn vault_telemetry_endpoint(
    _auth: ValidatedIdentity,
    State(state): State<ApiState>,
) -> Result<Json<VaultTelemetry>, ApiError> {
    let wal_pending_count = state.axon.get_total_leaves() as u64;

    let total_vectors = state.lance.get_total_vector_count().await.unwrap_or(0);

    let index_size_mb = state.lance.get_index_size_mb().await;

    let densest_namespace = state
        .lance
        .get_densest_namespace()
        .await
        .unwrap_or_else(|_| "Empty (0%)".to_string());

    let telemetry = VaultTelemetry {
        total_vectors,
        wal_pending_count,
        index_size_mb,
        densest_namespace,
        embedder_name: state.config.embedder_type.clone(),
        embeder_dim: state.lance.dims as usize,
    };

    Ok(Json(telemetry))
}

pub async fn active_qurantine_endpoint(
    _auth: ValidatedIdentity,
    State(state): State<ApiState>,
) -> Json<Vec<QuarantineRecord>> {
    let mut quarantined_agents = Vec::new();

    // Iterate over Dashmap Shards safely.
    for entry in state.aegis.quarantine_blocklist.iter() {
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
    state
        .global_net
        .dispatch_control_override(&payload.agent_hex, &payload.system_prompt_override)
        .await;

    // Unfreeze the Agent (Remove from DashhMap)
    if state
        .aegis
        .quarantine_blocklist
        .remove(&payload.agent_hex)
        .is_some()
    {
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
            .boot_historical_agent(&payload.agent_hex, None, None, false, state.phantom_ui_tx)
            .await
        {
            Ok(()) => Ok(Json(
                json!({"success": true, "message": "Quarantine lifted"}),
            )),

            Err(e) => {
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

    // 1. Lift aegis Quarantine so that the agent can actually boot
    // Unfreeze the Agent (Remove from DashhMap)
    if state
        .aegis
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
                state.phantom_ui_tx,
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

    let state_bytes = archived_ingress.state_bytes.as_slice();

    let archived_state =
        rkyv::access::<<AgentState as rkyv::Archive>::Archived, rkyv::rancor::Error>(state_bytes)
            .map_err(|e| {
            ApiError::BadRequest(format!("Malformed AgentState inner memory layout: {}", e))
        })?;

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
        .aegis
        .verify_session_lineage(&archived_ingress.capability_cert.as_slice(), &agent_pub_key)
        .map_err(|e| ApiError::Unauthorized(format!("Lineage Verification failed: {}", e)))?;

    let packet_timestamp: i64 = archived_state.timestamp.into();
    state
        .aegis
        .authorize_packet_fast(
            agent_hex.as_str(),
            group_name.as_str(),
            &agent_pub_key,
            &state_bytes,
            &packet_sig,
            &path_intent,
            packet_timestamp,
        )
        .map_err(|e| ApiError::Forbidden(format!("Aegis Interdiction: {}", e)))?;

    let task_event = state.event_tx.clone();
    let task_axon = state.axon.clone();
    let task_wal = state.wal.clone();
    let task_net = state.global_net.clone();
    let task_brain = state.brain.clone();

    let body_clone = body.clone();

    tokio::spawn(async move {
        // Recast the pointer inside the 'static task bounds
        let envelope = unsafe {
            rkyv::access_unchecked::<<IngressEnvelope as rkyv::Archive>::Archived>(&body_clone)
        };

        let state = unsafe {
            rkyv::access_unchecked::<<AgentState as rkyv::Archive>::Archived>(&envelope.state_bytes)
        };

        // Pass
        let res = execute_raqim_cascade(
            &state,
            task_axon,
            task_wal,
            task_brain,
            task_net,
            task_event,
            Vec::new(),
            Vec::new(),
        )
        .await;

        let _ = match res {
            Ok(id) => id,
            Err(_) => {
                eprintln!("[SECURITY FATAL] Unsigned/Anonymous payload hit the cascade. Dropped.");
                return;
            }
        };
    });

    Ok(Json(
        json!({"success": true, "message": "Thought accepted into cascade" }),
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
    let lance_future = state.lance.fetch_historical_timeline(&agent_hex);
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

#[derive(Serialize)]
pub struct DashboardCards {
    pub global_transactions: u64,
    pub active_agents: usize,
    pub vault_capacity: usize,
    pub hot_thoughts_count: u64,
    pub cold_thoughts_count: u64,
    pub latest_tx_hex: String,
    pub embedder_dims: i32,
    pub embedder_name: String,
    pub ingress_pause: bool,
}

pub async fn dashboard_cards_endpoint(
    _auth: ValidatedIdentity,
    State(state): State<ApiState>,
) -> Result<Json<DashboardCards>, ApiError> {
    let now = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs();

    // Iterate through the Dashmap
    let active_count = state
        .swarm_registry
        .active_agents
        .iter()
        .filter(|entry| {
            let is_recent = now.saturating_sub(entry.last_seen_ts) <= 60;
            let is_not_jailed = entry.status != "Quarantined";
            is_recent && is_not_jailed
        })
        .count();

    let cold_count = state.lance.get_total_vector_count().await.unwrap_or(0) as u64;
    let hot_batches = state.wal.get_pending_count().await as u64;

    // Hot buffer leaves + cold storage
    let hot_count = state.axon.get_total_leaves() as u64;
    let total_lifetime_txn = cold_count + hot_count;

    let latest_tx = state.axon.get_latest_tx_id();
    let latest_tx_hex = format!("0x{:032x}", latest_tx);

    Ok(Json(DashboardCards {
        global_transactions: total_lifetime_txn,
        active_agents: active_count,
        vault_capacity: cold_count as usize,
        hot_thoughts_count: hot_batches,
        cold_thoughts_count: cold_count,
        latest_tx_hex: latest_tx_hex,
        embedder_dims: state.lance.dims,
        embedder_name: state.config.embedder_type.clone(),
        ingress_pause: *state.pause_tx.borrow(),
    }))
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
    for entry in state.aegis.quarantine_blocklist.iter() {
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
    let policies_guard = state.aegis.group_policies.read();
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

    let node_id = state.global_net.os_node_id.clone();

    let highest_tx = state.axon.get_latest_tx_id();

    let cumulative_crdt_ops: usize = state
        .brain
        .shards
        .iter()
        .map(|e| e.value().doc.read().len_ops())
        .sum();

    let total_shards = state.brain.shards.len();

    let payload = json!({
        "node_id": node_id,
        "highest_tx_id": format!("0x{:032x}", highest_tx),
        "wal_bytes": wal_size,
        "wal_size_mb": (wal_size as f64 ) / (1024.0 * 1024.0),
        "buffer_load": pending_wal_items,
        "allocated_shards": total_shards,
        "cumulative_crdt_ops": cumulative_crdt_ops,
        "active_timelines": state.brain.shards.len(),
    });

    Ok(Json(payload))
}

/// Maps to  `raqim cluster topology`
pub async fn cluster_topology_endpoint(
    _auth: ValidatedIdentity,
    State(state): State<ApiState>,
) -> Result<Json<Value>, ApiError> {
    let mut shards = Vec::new();

    // Iterate through the DashMap of the active swarmbrain document
    for entry in state.brain.shards.iter() {
        let namespace = entry.key();
        let brain = entry.value();

        // Acquire a brief read lock on the Loro CRDT to extract topology metrics
        let doc_lock = brain.doc.read();

        // Count how many unique agent timelines exist within this specific shard
        let active_timelines = brain.root_timeline_map.len();

        // we measure the length of the underlying operations log.
        let ops_count = doc_lock.len_ops();

        // Calculate dynamic in-memoy footprint for this CRDT shard

        let estimated_ram_kb = (ops_count * 64) as f64 / 1024.0;
        let estimated_ram_mb = estimated_ram_kb / 1024.0;

        // Fetch agents associated with this namespace
        let attached_agents: Vec<String> = state
            .swarm_registry
            .active_agents
            .iter()
            .filter(|a| a.namespace == *namespace)
            .map(|a| a.key().clone())
            .collect();

        shards.push(json!({ "namespace": namespace, "active_timelines": active_timelines, "total_crdt_operation": ops_count, "estimated_ram_mb": (estimated_ram_mb * 100.0).round() / 100.0, "attached_agents": attached_agents, "status": "ACTIVE"  }));
    }

    Ok(Json(json!(shards)))
}

pub async fn cluster_enclaves_endpoint(
    _auth: ValidatedIdentity,
    State(state): State<ApiState>,
) -> Result<Json<Value>, ApiError> {
    let mut enclaves = Vec::new();

    // Collect currently active connected agents
    for entry in state.swarm_registry.active_agents.iter() {
        let agent = entry.value();
        enclaves.push(json!({
            "alias": agent.alias,
            "identity_hex": entry.agent_hex.clone(),
            "home_shard": agent.namespace.clone(),
            "status": agent.status,
            "last_seen_ts": agent.last_seen_ts

        }));
    }

    // If no live agents are connected, populate from indexed shard namespace
    if enclaves.is_empty() {
        for (i, entry) in state.brain.shards.iter().enumerate() {
            let namespace = entry.key();
            enclaves.push(json!({
                "alias": format!("agent_shard_{:02}", i),
                "identity_hex": format!("0x{:032x}", i + 1),
                "home_shard": namespace,
                "status": "IDLE_IN_RAM",
                "last_seen_ts": 0
            }));
        }
    }

    Ok(Json(json!(enclaves)))
}

#[derive(Deserialize)]
pub struct RecordEffectRequest {
    pub agent_hex: String,
    pub step_ordinal: u64,
    pub call_signature_hex: String,
    pub output_payload_base64: String,
    pub namespace: String,
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

    let call_signature_bytes = hex::decode(payload.call_signature_hex)
        .map_err(|_| ApiError::BadRequest("Invalid call_signature_hex format".to_string()))?;
    let call_signature_hash: [u8; 32] = call_signature_bytes.try_into().map_err(|_| {
        ApiError::BadRequest("call_signature_hex must be eactly 32 bytes".to_string())
    })?;

    let output_payload = base64::engine::general_purpose::STANDARD
        .decode(&payload.output_payload_base64)
        .map_err(|_| ApiError::BadRequest("Invalid base64 payload".to_string()))?;

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
            if is_forked {
                let tx_id = format!("{:032x}", tx_id);
                let timestamp = std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .unwrap_or_default()
                    .as_secs() as i64;

                let original_ns = payload.namespace.clone().replace("phantom_", "");
                // Audit Trail: Emitted to System Event Bus -> Persisted to lanceDB System event table
                let _ = state.event_tx.send(SystemEvent::RealityForked {
                    agent_id: payload.agent_hex.clone(),
                    original_namespace: original_ns.clone(),
                    phantom_namespace: payload.namespace.clone(),
                    step_ordinal: payload.step_ordinal,
                    tx_id: tx_id.clone(),
                    timestamp,
                });

                // Real-time Glass: Streamed to active SSE Firehose
                let _ = state.ui_tx.send(UiEvent::RealityForked {
                    agent_id: payload.agent_hex.clone(),
                    original_namespace: original_ns,
                    phantom_namespace: payload.namespace.clone(),
                    step_ordinal: payload.step_ordinal,
                    tx_id: tx_id.clone(),
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
                base64::engine::general_purpose::STANDARD.encode(&record.output_payload);

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
    match state.axon.generate_proof_for_tx(tx_id) {
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

// Route Builder
pub fn build_admin_router(state: ApiState) -> axum::Router {
    axum::Router::new()
        // State Proofs & Effect Recording
        .route("/v1/state/proof/:tx_id", get(get_state_proof_handler))
        .route("/v1/effect/record", post(record_effect_handler))
        .route("/v1/effect/get", post(get_effect_handler))
        // Aegis Firewall & Governance
        .route("/v1/aegis/quarantine_list", get(active_qurantine_endpoint))
        .route(
            "/v1/admin/quarantine/lift",
            post(lift_qurantine_and_resurrect),
        )
        .route("/v1/aegis/metrics", get(aegis_metics_endpoint))
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
        .route("/v1/admin/cluster/topology", get(cluster_topology_endpoint))
        .route("/v1/cluster/enclaves", get(cluster_enclaves_endpoint))
        .route("/v1/dashboard/cards", get(dashboard_cards_endpoint))
        .route("/v1/admin/ingress/toggle", post(toggle_ingress_endpoint))
        .route(
            "/v1/admin/compactor/trigger",
            post(trigger_compaction_endpoint),
        )
        // System & Agent Deployment endpoints
        .route("/v1/system/health/live", get(sse_health_endpoint))
        .route("/v1/system/firehose", get(sse_firehose_endpoint))
        .route("/v1/time-travel/stream", get(sse_phantom_endpoint))
        .route("/v1/system/agents/aliases", get(agent_alias_endpoint))
        // Swarm & A2A Ingress
        .route("/v1/mcp/ws", get(mcp_ws_handler))
        .route("/v1/swarm/ingress", post(http_ingress_endpoint))
        .route("/v1/swarm/memory", get(semantic_search_endpoint))
        .route("/v1/vault/search", get(unified_vault_search))
        .route("/v1/vault/telemetry", get(vault_telemetry_endpoint))
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
