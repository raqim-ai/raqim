use raqim_core::nucleus::WalEngine;
use raqim_core::{AgentState, AgentStatus, OpLog};
use std::path::Path;

#[tokio::test]
async fn test_wal_restarts_never_overites_and_compactor_reads_all_batches() {
    let test_wal_path = "test_run_integrity.wal";
    if Path::new(test_wal_path).exists() {
        let _ = std::fs::remove_file(test_wal_path);
    }

    // Boot WAL and write batch 1 (3 items)
    {
        let (wal, handle) = WalEngine::start(test_wal_path.to_string()).await;
        for i in 0..3 {
            let res = wal.append(create_mock_log(i, "Batch 2")).await;

            if res.is_err() {
                eprintln!("Durability Breach/WAL saturated");
                break;
            }
        }

        // Give tokio 20ms to flush group commit to disk
        tokio::time::sleep(tokio::time::Duration::from_millis(20)).await;
        drop(wal);
        let _ = handle.await;
    }

    let size_after_run1 = std::fs::metadata(test_wal_path).unwrap().len();
    assert!(size_after_run1 > 0, "WAL should contain bytes after run 1");

    // REBOOT SIMULATION: Boot WAL again and write batch 2 (3 items)
    {
        let (wal, handle) = WalEngine::start(test_wal_path.to_string()).await;
        for i in 3..6 {
            let res = wal.append(create_mock_log(i, "Batch 2")).await;

            if res.is_err() {
                eprintln!("Durability Breach/WAL saturated");
                break;
            }
        }

        tokio::time::sleep(tokio::time::Duration::from_millis(20)).await;

        drop(wal);
        let _ = handle.await;
    }

    let size_after_run2 = std::fs::metadata(test_wal_path).unwrap().len();

    // Proof 1: the file size must increase. If it overwrote, size wuld equal size_after_run1
    assert!(
        size_after_run2 > size_after_run1,
        "CRIT-02 REGRESSION: WAL did not append on reboot! File was overritten!"
    );

    // COMPACTOR SIMULATOR: Parse all frames from file
    let buffer = std::fs::read(test_wal_path).unwrap();
    let mut offset = 0;
    let mut total_baches_parsed = 0;

    while offset + 8 <= buffer.len() {
        let entry_len = u32::from_le_bytes(buffer[offset..offset + 4].try_into().unwrap()) as usize;
        let expected_crc = u32::from_le_bytes(buffer[offset + 4..offset + 8].try_into().unwrap());
        let frame_total = 8 + entry_len;

        let entry_slice = &buffer[offset + 8..offset + frame_total];
        let actual_crc = crc32fast::hash(entry_slice);

        assert_eq!(
            actual_crc,
            expected_crc,
            "CRIT-01 REGRESSION: CRC mismatch on batch {} at offset {}!",
            total_baches_parsed + 1,
            offset
        );

        total_baches_parsed += 1;
        offset += frame_total;
    }

    // Proof 2 Both batch must be extracted without offset drifts
    assert_eq!(
        total_baches_parsed, 2,
        "Compactor should have parsed exactly 2 distict batches"
    );
    let _ = std::fs::remove_file(test_wal_path);
}

fn create_mock_log(tx_id: u128, text: &str) -> OpLog {
    OpLog {
        agent_id: [1u8; 16],
        state: AgentState {
            agent_id: Some([1u8; 16]),
            transaction_id: tx_id,
            timestamp: 1700000000000000,
            status: AgentStatus::Idle,
            text: text.to_string(),
            namespace: "/test/verify".to_string(),
        },
        delta: vec![1, 2, 3],
        previous_hash: [0u8; 32],
        current_hash: [1u8; 32],
        entropy_seeds: vec![],
        network_responses: vec![],
    }
}
