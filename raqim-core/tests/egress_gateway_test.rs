use axum::body::Body;
use axum::http::{HeaderMap, Request, StatusCode};
use axum::response::{IntoResponse, Response};
use axum::routing::post;
use axum::{Json, Router};
use ed25519_dalek::SigningKey;
use rand_core::OsRng;
use raqim_core::api::{build_data_router, ApiState};
use raqim_core::cold_store::{ColdStore, SegmentColdStore};
use raqim_core::compactor::WalCompactor;
use raqim_core::config::{ConnectorConfig, RaqimConfig};
use raqim_core::effect_store::EffectStore;
use raqim_core::merkle::MerkleEngine;
use raqim_core::policy::{GroupPolicyConfig, JsonPredicate, PolicyConfigFile, PolicyEngine};
use raqim_core::registry::SwarmRegistry;
use raqim_core::token::mint_workload_token;
use raqim_core::wal::WalEngine;
use raqim_core::SystemEvent;
use serde_json::{json, Value};
use std::collections::HashMap;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;
use tokio::net::TcpListener;
use tokio::sync::{broadcast, watch};
use tower::ServiceExt;

const CANARY_SECRET: &str = "sk_live_CANARY_PAYSTACK_SECRET_DO_NOT_LEAK_998877";

#[tokio::test]
async fn test_egress_gateway_secret_injection_redaction_sse_replay_and_in_doubt() {
    let wal_path = "test_egress_phase1.wal";
    let journal_path = "test_egress_phase1_journal.bin";
    let checkpoint_path = "test_egress_phase1_ckpt.bin";
    let manifest_path = "test_egress_phase1_manifest.json";
    let cold_dir = "./test_egress_cold_phase1";

    let _ = std::fs::remove_file(wal_path);
    let _ = std::fs::remove_file(journal_path);
    let _ = std::fs::remove_file(checkpoint_path);
    let _ = std::fs::remove_file(manifest_path);
    let _ = std::fs::remove_dir_all(cold_dir);

    // Set the secret in the daemon's environment
    std::env::set_var("TEST_PAYSTACK_SECRET_ENV", CANARY_SECRET);

    // 1. Spawn a real local Mock Upstream Server (simulating Paystack + OpenAI SSE)
    let upstream_hits = Arc::new(AtomicUsize::new(0));
    let hits_clone = upstream_hits.clone();

    let mock_upstream = Router::new()
        .route(
            "/transfer",
            post(move |headers: HeaderMap, Json(body): Json<Value>| {
                let hits = hits_clone.clone();
                async move {
                    hits.fetch_add(1, Ordering::SeqCst);

                    // Verify Raqim injected the real secret and stripped the rqm_v1 token!
                    let auth = headers
                        .get("authorization")
                        .and_then(|v| v.to_str().ok())
                        .unwrap_or("");
                    assert_eq!(
                        auth,
                        format!("Bearer {}", CANARY_SECRET),
                        "Upstream did not receive injected secret!"
                    );

                    // Verify Raqim injected a non-empty 32-char UUIDv7 Idempotency-Key!
                    let idem = headers
                        .get("idempotency-key")
                        .and_then(|v| v.to_str().ok())
                        .unwrap_or("");
                    assert_eq!(
                        idem.len(),
                        32,
                        "Upstream must receive 32-hex UUIDv7 Idempotency-Key!"
                    );

                    // Verify the upstream still received the unredacted CVV in RAM
                    assert_eq!(body["card"]["cvv"], "999");

                    Json(json!({
                        "status": true,
                        "message": "Transfer queued",
                        "amount": body["amount"],
                        "card": { "cvv": "999" }
                    }))
                }
            }),
        )
        .route(
            "/v1/chat/completions",
            post(|| async {
                let sse_payload = "data: {\"choices\":[{\"delta\":{\"content\":\"Bismillah\"}}]}\n\ndata: [DONE]\n\n";
                let mut resp = Response::new(Body::from(sse_payload));
                resp.headers_mut().insert(
                    "content-type",
                    "text/event-stream".parse().unwrap(),
                );
                resp
            }),
        );

    let upstream_listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let upstream_addr = upstream_listener.local_addr().unwrap();
    tokio::spawn(async move {
        axum::serve(upstream_listener, mock_upstream).await.unwrap();
    });

    // 2. Configure Raqim with `paystack`, `openai`, and `broken_bank` (for In-Doubt test)
    let mut config = RaqimConfig::default();
    config.wal_path = wal_path.to_string();
    config.control_journal_path = journal_path.to_string();
    config.checkpoint_path = checkpoint_path.to_string();
    config.manifest_path = manifest_path.to_string();
    config.cold_store_path = cold_dir.to_string();
    config.connectors = vec![
        ConnectorConfig {
            name: "paystack".to_string(),
            upstream_url: format!("http://{}", upstream_addr),
            secret_env: "TEST_PAYSTACK_SECRET_ENV".to_string(),
            auth_header: "Authorization".to_string(),
            auth_prefix: "Bearer ".to_string(),
            idempotency_header: Some("Idempotency-Key".to_string()),
            allowed_routes: vec!["POST /transfer".to_string()],
            redact_pointers: vec!["/card/cvv".to_string()],
        },
        ConnectorConfig {
            name: "openai".to_string(),
            upstream_url: format!("http://{}", upstream_addr),
            secret_env: "TEST_PAYSTACK_SECRET_ENV".to_string(),
            auth_header: "Authorization".to_string(),
            auth_prefix: "Bearer ".to_string(),
            idempotency_header: None,
            allowed_routes: vec!["POST /v1/chat/completions".to_string()],
            redact_pointers: vec![],
        },
        ConnectorConfig {
            name: "broken_bank".to_string(),
            upstream_url: "http://127.0.0.1:1".to_string(), // Unreachable port -> simulates crash/drop mid-call!
            secret_env: "TEST_PAYSTACK_SECRET_ENV".to_string(),
            auth_header: "Authorization".to_string(),
            auth_prefix: "Bearer ".to_string(),
            idempotency_header: Some("Idempotency-Key".to_string()),
            allowed_routes: vec!["POST /wire".to_string()],
            redact_pointers: vec![],
        },
    ];
    let config = Arc::new(config);

    // 3. Initialize Raqim Core Engine & Policy
    let master_key = SigningKey::generate(&mut OsRng);
    let master_pub = master_key.verifying_key().to_bytes();

    let mut groups = HashMap::new();
    groups.insert(
        "fintech_group".to_string(),
        GroupPolicyConfig {
            allowed_namespaces: vec![
                "/egress/paystack/*".to_string(),
                "/egress/openai/*".to_string(),
                "/egress/broken_bank/*".to_string(),
            ],
            blocked_namespaces: vec![],
            max_tps: 100,
            burst_capacity: 50,
            predicates: vec![JsonPredicate {
                namespace: "/egress/paystack/transfer".to_string(),
                pointer: "/amount".to_string(),
                required: true,
                min_value: Some(100.0),
                max_value: Some(50_000.0),
                max_hourly_sum: Some(100_000.0),
                allowed_values: vec![],
            }],
        },
    );
    let policy_cfg = PolicyConfigFile { groups };
    let (event_tx, _event_rx) = broadcast::channel::<SystemEvent>(1000);

    let policy = Arc::new(PolicyEngine::new(
        policy_cfg.to_group_policies(),
        &master_pub,
        event_tx.clone(),
        journal_path.to_string(),
    ));
    let merkle = Arc::new(MerkleEngine::new());
    let (wal, wal_handle) = WalEngine::start(wal_path.to_string()).await;
    let cold_store: Arc<dyn ColdStore> = Arc::new(SegmentColdStore::new(cold_dir));
    let effect_store = Arc::new(EffectStore::new(
        config.clone(),
        merkle.clone(),
        cold_store.clone(),
        wal.clone(),
        event_tx.clone(),
    ));
    let registry = Arc::new(SwarmRegistry::new());
    let compactor = Arc::new(WalCompactor::new(
        wal_path,
        manifest_path,
        checkpoint_path,
        journal_path,
        cold_store.clone(),
        merkle.clone(),
        policy.clone(),
        registry.clone(),
        effect_store.clone(),
        event_tx.clone(),
        wal.cmd_sender.clone(),
    ));
    let (pause_tx, _) = watch::channel(false);

    let api_state = ApiState {
        config,
        effect_store: effect_store.clone(),
        merkle,
        policy,
        wal: wal.clone(),
        cold_store,
        event_tx,
        swarm_registry: registry,
        admin_secret: "test_admin_secret".to_string(),
        pause_tx: Arc::new(pause_tx),
        compactor,
    };

    let app = build_data_router(api_state);

    // Mint an `rqm_v1` workload token for our agent
    let agent_hex = "11223344556677889900aabbccddeeff";
    let workload_token =
        mint_workload_token(&master_key, agent_hex, "fintech_group", 3600).unwrap();

    // =========================================================================
    // TEST 1: Live Paystack Egress Call (Secret Injection + Redaction)
    // =========================================================================
    let req_body = br#"{"amount": 25000, "card": {"cvv": "999"}}"#;
    let req1 = Request::builder()
        .method("POST")
        .uri("/v1/egress/paystack/transfer")
        .header("authorization", format!("Bearer {}", workload_token))
        .header("content-type", "application/json")
        .body(Body::from(req_body.to_vec()))
        .unwrap();

    let resp1 = app.clone().oneshot(req1).await.unwrap();
    assert_eq!(resp1.status(), StatusCode::OK);
    assert_eq!(resp1.headers().get("x-raqim-replay").unwrap(), "MISS");
    assert_eq!(upstream_hits.load(Ordering::SeqCst), 1);

    // =========================================================================
    // TEST 2: CI Secret-Grep & CVV Redaction Audit on Disk Files
    // =========================================================================
    let wal_disk_bytes = std::fs::read(wal_path).unwrap();
    let wal_disk_str = String::from_utf8_lossy(&wal_disk_bytes);
    assert!(
        !wal_disk_str.contains(CANARY_SECRET),
        "SECURITY BREACH: Upstream secret leaked into WAL bytes on disk!"
    );
    assert!(
        !wal_disk_str.contains("\"999\""),
        "PRIVACY BREACH: Unredacted CVV '999' leaked into WAL bytes on disk!"
    );
    assert!(
        wal_disk_str.contains("[REDACTED_BY_POLICY]"),
        "WAL must contain '[REDACTED_BY_POLICY]' in place of '/card/cvv'"
    );

    // =========================================================================
    // TEST 3: Gateway-Level $0.00 Deterministic Replay (`X-Raqim-Mode: replay`)
    // =========================================================================
    let req_replay = Request::builder()
        .method("POST")
        .uri("/v1/egress/paystack/transfer")
        .header("authorization", format!("Bearer {}", workload_token))
        .header("x-raqim-mode", "replay")
        .header("content-type", "application/json")
        .body(Body::from(req_body.to_vec()))
        .unwrap();

    let resp_replay = app.clone().oneshot(req_replay).await.unwrap();
    assert_eq!(resp_replay.status(), StatusCode::OK);
    assert_eq!(resp_replay.headers().get("x-raqim-replay").unwrap(), "HIT");
    // Verify upstream_hits is STILL 1 (upstream was never called during replay!)
    assert_eq!(
        upstream_hits.load(Ordering::SeqCst),
        1,
        "Replay mode must not contact the upstream server!"
    );

    // =========================================================================
    // TEST 4: Zero-Stall SSE Streaming Byte-Tee & Stream Replay
    // =========================================================================
    let sse_req = Request::builder()
        .method("POST")
        .uri("/v1/egress/openai/v1/chat/completions")
        .header("authorization", format!("Bearer {}", workload_token))
        .header("content-type", "application/json")
        .body(Body::from(br#"{"model":"gpt-4o","stream":true}"#.to_vec()))
        .unwrap();

    let sse_resp = app.clone().oneshot(sse_req).await.unwrap();
    assert_eq!(sse_resp.status(), StatusCode::OK);
    let streamed_bytes = axum::body::to_bytes(sse_resp.into_body(), 1024 * 1024)
        .await
        .unwrap();
    assert!(String::from_utf8_lossy(&streamed_bytes).contains("Bismillah"));

    // Replay the SSE stream for $0.00!
    let sse_replay_req = Request::builder()
        .method("POST")
        .uri("/v1/egress/openai/v1/chat/completions")
        .header("authorization", format!("Bearer {}", workload_token))
        .header("x-raqim-mode", "replay")
        .header("content-type", "application/json")
        .body(Body::from(br#"{"model":"gpt-4o","stream":true}"#.to_vec()))
        .unwrap();

    let sse_replay_resp = app.clone().oneshot(sse_replay_req).await.unwrap();
    assert_eq!(
        sse_replay_resp.headers().get("x-raqim-replay").unwrap(),
        "HIT"
    );
    let replayed_sse_bytes = axum::body::to_bytes(sse_replay_resp.into_body(), 1024 * 1024)
        .await
        .unwrap();
    assert_eq!(streamed_bytes, replayed_sse_bytes);

    // =========================================================================
    // TEST 5: Mid-Call Connection Failure -> Surfaces as `IN_DOUBT` Intent!
    // =========================================================================
    assert_eq!(effect_store.get_in_doubt_intents().len(), 0);

    let broken_req = Request::builder()
        .method("POST")
        .uri("/v1/egress/broken_bank/wire")
        .header("authorization", format!("Bearer {}", workload_token))
        .header("content-type", "application/json")
        .body(Body::from(br#"{"amount": 5000}"#.to_vec()))
        .unwrap();

    let broken_resp = app.oneshot(broken_req).await.unwrap();
    assert_eq!(broken_resp.status(), StatusCode::INTERNAL_SERVER_ERROR);

    let in_doubt = effect_store.get_in_doubt_intents();
    assert_eq!(
        in_doubt.len(),
        1,
        "Failed mid-flight upstream call MUST leave its Write-Ahead Intent in IN_DOUBT!"
    );
    assert_eq!(in_doubt[0].namespace, "/egress/broken_bank/wire");

    // Cleanup
    let _ = wal
        .cmd_sender
        .send(raqim_core::wal::WalCommand::Shutdown)
        .await;
    drop(wal);
    let _ = wal_handle.await;
    let _ = std::fs::remove_file(wal_path);
    let _ = std::fs::remove_file(journal_path);
    let _ = std::fs::remove_file(checkpoint_path);
    let _ = std::fs::remove_file(manifest_path);
    let _ = std::fs::remove_dir_all(cold_dir);
}
