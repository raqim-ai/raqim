use raqim_core::merkle::MerkleEngine;
use raqim_core::{ExecutionRecord, ExecutionStage};

#[test]
fn test_odd_leaf_duplication_collision_resistance() {
    let a = [1u8; 32];
    let b = [2u8; 32];
    let c = [3u8; 32];

    let root_3 = MerkleEngine::compute_merkle_root(&[a, b, c]);
    let root_4 = MerkleEngine::compute_merkle_root(&[a, b, c, c]);

    assert_ne!(
        root_3, root_4,
        "CVE-2012-2459 REGRESSION: 3-leaf tree [a,b,c] collided with 4-leaf tree [a,b,c,c]!"
    );
}

#[test]
fn test_envelope_commitment_blocks_stage_and_metadata_tampering() {
    let engine = MerkleEngine::new();
    let agent_id = [7u8; 16];

    for i in 0..5 {
        let rec = ExecutionRecord::new(
            agent_id,
            i,
            format!("payload_{}", i).into_bytes(),
            if i == 2 {
                ExecutionStage::Denied
            } else {
                ExecutionStage::Outcome
            },
            "/finance/transfer".to_string(),
            [i as u8; 32],
        );
        let _ = engine.seal_record(rec);
    }

    // Retrieve target record #2 (which was Denied)
    let buf = engine.active_buffers.get("/finance/transfer").unwrap();
    let rec2 = buf.read().accumulated_records[2].clone();
    drop(buf);

    let proof = engine
        .generate_proof_for_tx(rec2.tx_id)
        .expect("Proof must exist");

    // 1. Untampered verification passes
    assert!(
        MerkleEngine::verify_inclusion(&rec2.payload_bytes, &agent_id, &proof),
        "Untampered record must pass offline verification"
    );

    // 2. Tamper payload by 1 byte -> MUST FAIL
    let mut bad_payload = rec2.payload_bytes.clone();
    bad_payload[0] ^= 0xFF;
    assert!(
        !MerkleEngine::verify_inclusion(&bad_payload, &agent_id, &proof),
        "Modified payload must fail verification"
    );

    // 3. Flip stage from Denied (3) to Outcome (2) in proof metadata -> MUST FAIL
    let mut forged_stage_proof = proof.clone();
    forged_stage_proof.stage = ExecutionStage::Outcome as u8;
    assert!(
        !MerkleEngine::verify_inclusion(&rec2.payload_bytes, &agent_id, &forged_stage_proof),
        "Flipping Denied -> Outcome must fail Merkle envelope verification!"
    );

    // 4. Flush active buffer to crystallized batch archive (Case B) and verify proof still works!
    let _ = engine.flush_all_active_buffers();
    let archived_proof = engine
        .generate_proof_for_tx(rec2.tx_id)
        .expect("Crystallized batch proof (Case B) must exist");
    assert!(!archived_proof.is_active_buffer);
    assert!(
        MerkleEngine::verify_inclusion(&rec2.payload_bytes, &agent_id, &archived_proof),
        "Case B crystallized batch proof must verify with full LeafMetadata!"
    );
}
