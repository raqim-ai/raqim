use raqim_core::aegis::QuarantineRecord;
use raqim_core::api::TimelineNode;
use raqim_core::axon::MarkleBatch;
use raqim_core::checkpoint::{
    ActiveTreeBufferSnapshot, AxonCheckpointState, CheckpointEngine, ControlMutation,
    StateCheckpoint,
};
use raqim_core::otel::{build_otlp_trace_from_timeline, OtelAnyValue};
use raqim_core::registry::AgentProcess;
use raqim_core::EffectRecord;
use std::collections::HashMap;
use std::path::Path;

#[test]
fn test_checkpoint_and_control_journal_roundtrip() {
    let checkpoint_path = Path::new("./vault/test_smoke_checkpoint.bin");
    let journal_path = Path::new("./vault/test_smoke_journal.bin");

    // Clean up any leftovers
    if checkpoint_path.exists() {
        let _ = std::fs::remove_file(checkpoint_path);
    }
    if journal_path.exists() {
        let _ = std::fs::remove_file(journal_path);
    }

    // 1. Construct Mock Checkpoint
    let mut crdt_snapshots = HashMap::new();
    crdt_snapshots.insert("swarm_alpha".to_string(), vec![0xDE, 0xAD, 0xBE, 0xEF]);

    let axon_state = AxonCheckpointState {
        global_batch_counter: 42,
        active_buffers: vec![ActiveTreeBufferSnapshot {
            namespace: "swarm_alpha".to_string(),
            current_batch_id: 42,
            parent_batch_root: [1u8; 32],
            accumulated_leaves: vec![[2u8; 32]],
            accumulated_logs: Vec::new(),
            accumulated_tx_ids: vec![1001],
        }],
        batch_archive: vec![MarkleBatch {
            batch_id: 41,
            namespace: "swarm_alpha".to_string(),
            markle_root: [3u8; 32],
            parent_batch_root: [0u8; 32],
            leaves: vec![[4u8; 32]],
        }],
    };

    let effects = vec![EffectRecord {
        agent_id: [9u8; 16],
        step_ordinal: 3,
        call_signature_hash: [7u8; 32],
        output_payload: b"Mock Tool Output".to_vec(),
        transaction_id: 8888,
        timestamp: 1727160000,
    }];

    let quarantines = vec![QuarantineRecord {
        agent_hex: "deadbeef00112233".to_string(),
        violation_type: "UNAUTHORIZED_FILE_WRITE".to_string(),
        attemped_path: "/etc/shadow".to_string(),
        payload_preview: "echo root > /etc/shadow".to_string(),
        timestamp: 1727160000,
    }];

    let active_agents = vec![AgentProcess {
        agent_hex: "deadbeef00112233".to_string(),
        alias: "rogue_bot".to_string(),
        namespace: "swarm_alpha".to_string(),
        last_seen_ts: 1727160000,
        status: "Quarantined".to_string(),
    }];

    let checkpoint = StateCheckpoint {
        version: 1,
        timestamp: 1727160000,
        last_tx_id: 8888,
        crdt_snapshots,
        axon_state,
        effects,
        quarantines,
        active_agents,
    };

    // 2. Persist Checkpoint Atomically
    CheckpointEngine::write_checkpoint_atomically(checkpoint_path, &checkpoint)
        .expect("Checkpoint write failed");
    assert!(
        checkpoint_path.exists(),
        "Checkpoint file must exist on disk"
    );

    // 3. Load Checkpoint and verify CRC32 and fields
    let loaded = CheckpointEngine::load_checkpoint(checkpoint_path)
        .expect("Failed to load written checkpoint");

    assert_eq!(loaded.version, 1);
    assert_eq!(loaded.last_tx_id, 8888);
    assert_eq!(
        loaded.crdt_snapshots.get("swarm_alpha").unwrap(),
        &vec![0xDE, 0xAD, 0xBE, 0xEF]
    );
    assert_eq!(loaded.axon_state.global_batch_counter, 42);
    assert_eq!(loaded.effects.len(), 1);
    assert_eq!(loaded.effects[0].step_ordinal, 3);
    assert_eq!(loaded.quarantines.len(), 1);
    assert_eq!(loaded.quarantines[0].agent_hex, "deadbeef00112233");
    assert_eq!(loaded.active_agents.len(), 1);
    assert_eq!(loaded.active_agents[0].alias, "rogue_bot");

    // 4. Test Control Journal Append
    let mutation1 = ControlMutation::Quarantine(QuarantineRecord {
        agent_hex: "cafebebe44556677".to_string(),
        violation_type: "RATE_LIMIT_EXCEEDED".to_string(),
        attemped_path: "/api/pay".to_string(),
        payload_preview: "transfer()".to_string(),
        timestamp: 1727160100,
    });
    let mutation2 = ControlMutation::LiftQuarantine {
        agent_hex: "deadbeef00112233".to_string(),
    };

    CheckpointEngine::append_control_mutation(journal_path, &mutation1)
        .expect("Append mutation 1 failed");
    CheckpointEngine::append_control_mutation(journal_path, &mutation2)
        .expect("Append mutation 2 failed");

    // 5. Replay Journal
    let replayed = CheckpointEngine::replay_control_journal(journal_path);
    assert_eq!(replayed.len(), 2, "Expected 2 journal mutations");
    match &replayed[0] {
        ControlMutation::Quarantine(rec) => assert_eq!(rec.agent_hex, "cafebebe44556677"),
        _ => panic!("Expected Quarantine mutation"),
    }
    match &replayed[1] {
        ControlMutation::LiftQuarantine { agent_hex } => {
            assert_eq!(agent_hex, "deadbeef00112233")
        }
        _ => panic!("Expected LiftQuarantine mutation"),
    }

    // 6. Truncate Journal
    CheckpointEngine::truncate_control_journal(journal_path);
    assert!(
        !journal_path.exists(),
        "Journal must be removed upon truncation"
    );

    // Clean up
    let _ = std::fs::remove_file(checkpoint_path);
}

#[test]
fn test_otel_projection_and_ordinal_parsing() {
    let nodes = vec![
        TimelineNode {
            tx_id: 0x1234_5678_9ABC_DEF0,
            timestamp: "1727160000".to_string(), // seconds
            agent_status: "TOOL_EXEC".to_string(),
            payload_preview: "[STEP 1 EFFECT] HTTP GET https://api.stripe.com/v1/charges"
                .to_string(),
        },
        TimelineNode {
            tx_id: 0x1234_5678_9ABC_DEF1,
            timestamp: "1727160005500".to_string(), // milliseconds
            agent_status: "REASONING".to_string(),
            payload_preview: "[STEP 2 EFFECT] Summarize financial audit".to_string(),
        },
    ];

    let payload = build_otlp_trace_from_timeline(
        "agent_test_hex",
        "tenant_finance",
        "node_master",
        "00112233445566778899aabbccddeeff",
        &nodes,
    );

    let resource_spans = &payload.resource_spans;
    assert_eq!(resource_spans.len(), 1);
    let scope_spans = &resource_spans[0].scope_spans;
    assert_eq!(scope_spans.len(), 1);
    let spans = &scope_spans[0].spans;
    assert_eq!(spans.len(), 2);

    // Verify TraceID is 32-hex characters
    assert_eq!(spans[0].trace_id.len(), 32);
    assert_eq!(spans[0].trace_id, spans[1].trace_id); // Same session shares TraceID

    // Verify Ordinal Extraction
    assert_eq!(spans[0].name, "Step 1: TOOL_EXEC");
    assert_eq!(spans[1].name, "Step 2: REASONING");

    // Verify Timestamps (seconds -> 1e9, ms -> 1e6)
    assert!(
        spans[0].start_time_unix_nano.ends_with("000000000"),
        "Seconds must be scaled to nanoseconds"
    );
    assert!(
        spans[1].start_time_unix_nano.ends_with("000000"),
        "Milliseconds must be scaled to nanoseconds"
    );

    // Verify Step Ordinal Attributes
    let step_0_attr = spans[0]
        .attributes
        .iter()
        .find(|a| a.key == "raqim.step_ordinal")
        .expect("raqim.step_ordinal attribute must exist");
    match step_0_attr.value {
        OtelAnyValue::IntValue(v) => assert_eq!(v, 1),
        _ => panic!("Expected OtelAnyValue::IntValue for step_0_attr"),
    }

    let step_1_attr = spans[1]
        .attributes
        .iter()
        .find(|a| a.key == "raqim.step_ordinal")
        .expect("raqim.step_ordinal attribute must exist");
    match step_1_attr.value {
        OtelAnyValue::IntValue(v) => assert_eq!(v, 2),
        _ => panic!("Expected OtelAnyValue::IntValue for step_1_attr"),
    }
}
