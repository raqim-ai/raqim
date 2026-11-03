use ed25519_dalek::SigningKey;
use rand_core::OsRng;
use raqim_core::policy::{
    GroupPolicyConfig, JsonPredicate, PolicyConfigFile, PolicyEngine, PolicyError,
};
use raqim_core::token::{mint_workload_token, verify_workload_token};
use raqim_core::SystemEvent;
use std::collections::HashMap;
use tokio::sync::broadcast;

#[test]
fn test_workload_token_mint_verify_and_tamper_rejection() {
    let master_key = SigningKey::generate(&mut OsRng);
    let master_pub = master_key.verifying_key();

    let token = mint_workload_token(
        &master_key,
        "00112233445566778899aabbccddeeff",
        "fintech_group",
        3600,
    )
    .expect("Token mint failed");
    assert!(token.starts_with("rqm_v1."));

    // 1. Valid verification
    let claims = verify_workload_token(&token, &master_pub).expect("Valid token must verify");
    assert_eq!(claims.sub, "00112233445566778899aabbccddeeff");
    assert_eq!(claims.grp, "fintech_group");

    // 2. Tamper with 1 character in the payload segment -> Must fail before JSON parsing!
    let mut parts: Vec<&str> = token.split('.').collect();
    let mutated_payload = format!("{}A", parts[1]);
    parts[1] = &mutated_payload;
    let forged_token = parts.join(".");
    assert!(
        matches!(
            verify_workload_token(&forged_token, &master_pub),
            Err(PolicyError::Unauthenticated(_))
        ),
        "Tampered token must be rejected as Unauthenticated"
    );
}

#[test]
fn test_four_concentric_egress_locks_and_anti_smuggling() {
    let journal_path = "test_egress_locks_journal.bin";
    let _ = std::fs::remove_file(journal_path);

    let master_key = SigningKey::generate(&mut OsRng);
    let master_pub = master_key.verifying_key().to_bytes();

    let mut groups = HashMap::new();
    groups.insert(
        "fintech_agent".to_string(),
        GroupPolicyConfig {
            allowed_namespaces: vec!["/egress/paystack/*".to_string()],
            blocked_namespaces: vec![],
            max_tps: 100,
            burst_capacity: 50,
            predicates: vec![
                JsonPredicate {
                    namespace: "/egress/paystack/transfer".to_string(),
                    pointer: "/amount".to_string(),
                    required: true,
                    min_value: Some(100.0),
                    max_value: Some(50_000.0),       // Max 50k per transfer
                    max_hourly_sum: Some(100_000.0), // Max 100k per hour
                    allowed_values: vec![],
                },
                JsonPredicate {
                    namespace: "/egress/paystack/transfer".to_string(),
                    pointer: "/currency".to_string(),
                    required: true,
                    min_value: None,
                    max_value: None,
                    max_hourly_sum: None,
                    allowed_values: vec!["NGN".to_string()],
                },
            ],
        },
    );

    let cfg = PolicyConfigFile { groups };
    let (tx, _rx) = broadcast::channel::<SystemEvent>(100);
    let engine = PolicyEngine::new(
        cfg.to_group_policies(),
        &master_pub,
        tx,
        journal_path.to_string(),
    );

    let allowed_routes = vec!["POST /transfer".to_string()];

    // 1. Valid polite transfer #1 (40,000 NGN) -> PASSES
    let valid_body = br#"{"amount": 40000, "currency": "NGN", "recipient": "RCP_01"}"#;
    let res1 = engine.authorize_egress_request(
        "agent_01",
        "fintech_agent",
        "POST",
        "/transfer",
        "/egress/paystack/transfer",
        &allowed_routes,
        valid_body,
    );
    assert!(res1.is_ok(), "Valid 40k NGN transfer should be allowed");

    // 2. Valid polite transfer #2 (45,000 NGN -> cumulative 85,000 NGN <= 100k cap) -> PASSES
    let body2 = br#"{"amount": 45000, "currency": "NGN", "recipient": "RCP_01"}"#;
    let res2 = engine.authorize_egress_request(
        "agent_01",
        "fintech_agent",
        "POST",
        "/transfer",
        "/egress/paystack/transfer",
        &allowed_routes,
        body2,
    );
    assert!(
        res2.is_ok(),
        "Second transfer (85k cumulative) should be allowed"
    );

    // 3. LOCK 3 TEST: Polite transfer #3 (30,000 NGN -> cumulative 115,000 NGN > 100k hourly cap!)
    // Must trip VELOCITY_BUDGET_BREACH and quarantine `agent_01`!
    let body3 = br#"{"amount": 30000, "currency": "NGN", "recipient": "RCP_01"}"#;
    let res3 = engine.authorize_egress_request(
        "agent_01",
        "fintech_agent",
        "POST",
        "/transfer",
        "/egress/paystack/transfer",
        &allowed_routes,
        body3,
    );
    assert!(matches!(res3, Err(PolicyError::ForbiddenNamespace(_))));
    assert!(
        engine.is_quarantined("agent_01"),
        "Exceeding rolling 1h velocity budget MUST quarantine agent_01!"
    );

    // 4. LOCK 4 TEST: JSON Duplicate-Key Smuggling Attack on `agent_02`
    let smuggled_body = br#"{"amount": 1000, "amount": 50000000, "currency": "NGN"}"#;
    let res_smuggle = engine.authorize_egress_request(
        "agent_02",
        "fintech_agent",
        "POST",
        "/transfer",
        "/egress/paystack/transfer",
        &allowed_routes,
        smuggled_body,
    );
    assert!(matches!(
        res_smuggle,
        Err(PolicyError::ForbiddenNamespace(_))
    ));
    assert!(
        engine.is_quarantined("agent_02"),
        "Duplicate JSON key smuggling MUST quarantine agent_02!"
    );

    // 5. LOCK 2 TEST: Single-transfer cap breach (500,000 NGN > 50,000 max_value) on `agent_03`
    let over_cap_body = br#"{"amount": 500000, "currency": "NGN"}"#;
    let res_cap = engine.authorize_egress_request(
        "agent_03",
        "fintech_agent",
        "POST",
        "/transfer",
        "/egress/paystack/transfer",
        &allowed_routes,
        over_cap_body,
    );
    assert!(matches!(res_cap, Err(PolicyError::ForbiddenNamespace(_))));
    assert!(
        engine.is_quarantined("agent_03"),
        "Exceeding max_value predicate MUST quarantine agent_03!"
    );

    let _ = std::fs::remove_file(journal_path);
}
