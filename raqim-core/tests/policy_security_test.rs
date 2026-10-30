use ed25519_dalek::{Signer, SigningKey};
use rand_core::OsRng;
use raqim_core::policy::{
    CapabilityCertificate, GroupPolicyConfig, PolicyConfigFile, PolicyEngine, PolicyError,
    MAX_CONSECUTIVE_RATE_STRIKES,
};
use raqim_core::SystemEvent;
use std::collections::HashMap;
use std::time::{SystemTime, UNIX_EPOCH};
use tokio::sync::broadcast;

#[test]
fn test_unauthenticated_spoof_never_quarantines_honest_agent_and_two_tier_rate_limit() {
    let journal_path = "test_policy_security_journal.bin";
    let _ = std::fs::remove_file(journal_path);

    let master_key = SigningKey::generate(&mut OsRng);
    let master_pub = master_key.verifying_key().to_bytes();

    let agent_key = SigningKey::generate(&mut OsRng);
    let agent_pub = agent_key.verifying_key().to_bytes();

    let mut hasher = blake3::Hasher::new_derive_key("raqim.agent.v1.identity");
    hasher.update(&agent_pub);
    let mut agent_id_bytes = [0u8; 16];
    hasher.finalize_xof().fill(&mut agent_id_bytes);
    let agent_hex = hex::encode(agent_id_bytes);

    // Mint valid capability certificate for agent_hex
    let now_ts = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_secs();
    let mut cert = CapabilityCertificate {
        agent_hex: agent_hex.clone(),
        group_name: "worker_group".to_string(),
        expiration_timestamp: now_ts + 3600,
        master_signature: Vec::new(),
    };
    let unsigned_bytes = postcard::to_allocvec(&cert).unwrap();
    cert.master_signature = master_key.sign(&unsigned_bytes).to_bytes().to_vec();

    // Policy: burst_capacity = 3, max_tps = 0 (no refill during test)
    let mut groups = HashMap::new();
    groups.insert(
        "worker_group".to_string(),
        GroupPolicyConfig {
            allowed_namespaces: vec!["/finance/screen/*".to_string()],
            blocked_namespaces: vec!["/finance/restricted/*".to_string()],
            max_tps: 0,
            burst_capacity: 3,
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

    // 1. Attacker sends forged signature claiming to be `agent_hex` -> Must return Unauthenticated (401)
    // and MUST NOT quarantine the honest `agent_hex`!
    let bad_sig = [0u8; 64];
    let res = engine.authorize_packet_fast(
        &agent_hex,
        "worker_group",
        &agent_pub,
        b"legit_payload",
        &bad_sig,
        "/finance/screen/tx",
        now_ts as i64,
    );
    assert!(matches!(res, Err(PolicyError::Unauthenticated(_))));
    assert!(
        !engine.is_quarantined(&agent_hex),
        "CRITICAL DO-S BUG: Unauthenticated spoofed signature quarantined honest agent!"
    );

    // 2. Honest agent signs 3 valid requests (consumes burst_capacity = 3)
    let payload = b"legit_payload";
    let valid_sig = agent_key.sign(payload).to_bytes();
    for _ in 0..3 {
        engine
            .authorize_packet_fast(
                &agent_hex,
                "worker_group",
                &agent_pub,
                payload,
                &valid_sig,
                "/finance/screen/tx",
                now_ts as i64,
            )
            .expect("Valid request within burst capacity must succeed");
    }

    // 3. 4th request hits rate limit -> Must return RateLimited (429), NOT Quarantine!
    let rate_res = engine.authorize_packet_fast(
        &agent_hex,
        "worker_group",
        &agent_pub,
        payload,
        &valid_sig,
        "/finance/screen/tx",
        now_ts as i64,
    );
    assert!(matches!(rate_res, Err(PolicyError::RateLimited(_))));
    assert!(
        !engine.is_quarantined(&agent_hex),
        "Transient rate limit burst must return 429, not quarantine!"
    );

    // 4. Sustained runaway flood (49 more consecutive rate violations = 50 total) -> Quarantines!
    for strike in 2..MAX_CONSECUTIVE_RATE_STRIKES {
        let r = engine.authorize_packet_fast(
            &agent_hex,
            "worker_group",
            &agent_pub,
            payload,
            &valid_sig,
            "/finance/screen/tx",
            now_ts as i64,
        );
        assert!(
            matches!(r, Err(PolicyError::RateLimited(_))),
            "Strike {} should be 429",
            strike
        );
    }

    let flood_res = engine.authorize_packet_fast(
        &agent_hex,
        "worker_group",
        &agent_pub,
        payload,
        &valid_sig,
        "/finance/screen/tx",
        now_ts as i64,
    );
    assert!(matches!(flood_res, Err(PolicyError::Quarantined(_))));
    assert!(
        engine.is_quarantined(&agent_hex),
        "50th consecutive rate strike must quarantine!"
    );

    let _ = std::fs::remove_file(journal_path);
}
