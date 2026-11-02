use raqim_core::checkpoint::{CheckpointEngine, ControlMutation};
use raqim_core::cold_store::{ColdStore, SegmentColdStore};
use raqim_core::policy::QuarantineRecord;
use raqim_core::{ExecutionRecord, ExecutionStage};
use std::path::Path;

#[tokio::test]
async fn test_cold_store_o1_index_and_in_place_ndpa_redaction() {
    let test_dir = "./test_cold_segments_phase0";
    let _ = std::fs::remove_dir_all(test_dir);

    let store = SegmentColdStore::new(test_dir);
    let agent_id = [9u8; 16];

    let rec1 = ExecutionRecord::new(
        agent_id,
        1,
        "/finance/kyc".to_string(),
        ExecutionStage::Outcome,
        [1u8; 32],
        b"BVN:22345678901".to_vec(),
    );
    let rec2 = ExecutionRecord::new(
        agent_id,
        2,
        "/finance/kyc".to_string(),
        ExecutionStage::Outcome,
        [2u8; 32],
        b"TRANSFER_APPROVED".to_vec(),
    );

    let tx1 = rec1.tx_id;
    let tx2 = rec2.tx_id;
    let orig_payload_hash_1 = rec1.payload_hash;

    store
        .archive_batch(&[rec1, rec2])
        .await
        .expect("Archive batch failed");

    // 1. Verify O(1) point lookup for both records
    let fetched1 = store.fetch_by_txid(tx1).await.unwrap().unwrap();
    assert_eq!(fetched1.payload_bytes, b"BVN:22345678901");

    // 2. Execute NDPA in-place redaction on Record 1
    let redacted = store.redact_by_txid(tx1).await.expect("Redaction failed");
    assert!(redacted, "Record 1 should be marked redacted");

    // 3. Verify Record 1's PII and salt are zeroed on disk, while payload_hash is preserved!
    let post_redact_1 = store.fetch_by_txid(tx1).await.unwrap().unwrap();
    assert!(
        post_redact_1.is_redacted(),
        "Record 1 must report is_redacted()"
    );
    assert_eq!(post_redact_1.payload_salt, [0u8; 16], "Salt must be zeroed");
    assert_eq!(
        post_redact_1.payload_hash, orig_payload_hash_1,
        "payload_hash must remain intact for Merkle verification!"
    );
    assert_eq!(post_redact_1.payload_text_preview(), "[REDACTED_NDPA]");

    // 4. Verify Record 2 (located AFTER Record 1 in the segment file) still reads cleanly at its original byte offset!
    let post_redact_2 = store.fetch_by_txid(tx2).await.unwrap().unwrap();
    assert_eq!(
        post_redact_2.payload_bytes, b"TRANSFER_APPROVED",
        "In-place redaction of Record 1 must not corrupt offset of Record 2!"
    );

    let _ = std::fs::remove_dir_all(test_dir);
}

#[test]
fn test_atomic_journal_rotation_preserves_concurrent_quarantines() {
    let journal_path = Path::new("test_atomic_rotation_journal.bin");
    let _ = std::fs::remove_file(journal_path);
    let _ = std::fs::remove_file(journal_path.with_extension("bin.compacting"));

    // 1. Write mutation A before compaction starts
    let q1 = ControlMutation::Quarantine(QuarantineRecord {
        agent_hex: "agent_alpha".to_string(),
        violation_type: "NAMESPACE_BREACH".to_string(),
        attemped_path: "/admin".to_string(),
        payload_preview: "test1".to_string(),
        timestamp: 100,
    });
    CheckpointEngine::append_control_mutation(journal_path, &q1).unwrap();

    // 2. Compactor rotates journal to `.compacting` before snapshotting RAM
    let rotated = CheckpointEngine::rotate_journal_before_snapshot(journal_path)
        .expect("Journal rotation should return .compacting path");

    // 3. While checkpoint is writing, a new quarantine (q2) arrives!
    let q2 = ControlMutation::Quarantine(QuarantineRecord {
        agent_hex: "agent_beta".to_string(),
        violation_type: "NAMESPACE_BREACH".to_string(),
        attemped_path: "/vault".to_string(),
        payload_preview: "test2".to_string(),
        timestamp: 101,
    });
    CheckpointEngine::append_control_mutation(journal_path, &q2).unwrap();

    // 4. Compactor finishes checkpoint and deletes ONLY `.compacting`
    let _ = std::fs::remove_file(&rotated);

    // 5. On reboot, `q2` is still safely inside `control_journal.bin`!
    let replayed = CheckpointEngine::replay_control_journal(journal_path);
    assert_eq!(
        replayed.len(),
        1,
        "Concurrent mutation during compaction must not be lost!"
    );
    match &replayed[0] {
        ControlMutation::Quarantine(r) => assert_eq!(r.agent_hex, "agent_beta"),
        _ => panic!("Expected agent_beta quarantine"),
    }

    let _ = std::fs::remove_file(journal_path);
}
