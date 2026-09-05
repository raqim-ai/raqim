use raqim_core::{AgentState, AgentStatus, OpLog, memory_router::MemoryRouter, nucleus::WalEngine};
use std::path::Path;

#[tokio::test]
async fn test_wal_scanner_reads_batch_frames_without_ub() {
    let test_wal = "test_deep_scan.wal";
    if Path::new(test_wal).exists() {
        let _ = std::fs::remove_file(test_wal);
    }

    // Boot WAL and commit a batch of 5 logs
    {
        let (wal, handle) = WalEngine::start(test_wal.to_string()).await;
        let mut count = 0;
        for i in 1..=5 {
            let log = create_test_log(i, &format!("Thought number {}", i));
            let res = wal.append(create_test_log(i, "Batch 2")).await;

            if res.is_err() {
                eprintln!("Durability Breach/WAL saturated");
                break;
            }
            count += 1;
        }

        tokio::time::sleep(tokio::time::Duration::from_millis(25)).await;
        drop(wal);
        let _ = handle.await;
        println!();
    }

    // Scan using the corrected zero-copy scanner;
    let mut scanned_count = 0;
    let mut tx_ids = Vec::new();

    let scan_res = MemoryRouter::scan_wal_file(test_wal, |archived_log| {
        scanned_count += 1;
        let tx = archived_log.state.transaction_id.to_native();
        tx_ids.push(tx);
    });

    assert!(scan_res.is_ok(), "Scanner failed with an error!");
    assert_eq!(
        scanned_count, 5,
        "CRIT-03 REGRESSION: Did not extract 5 logs from batch"
    );

    assert_eq!(
        tx_ids,
        vec![1, 2, 3, 4, 5],
        "CRIT-03 REGRESSION: Transaction IDs corruped!",
    );

    let _ = std::fs::remove_file(test_wal);
}

fn create_test_log(tx_id: u128, text: &str) -> OpLog {
    OpLog {
        agent_id: [7u8; 16],
        state: AgentState {
            agent_id: Some([7u8; 16]),
            transaction_id: tx_id,
            timestamp: 1700000000,
            status: AgentStatus::Idle,
            text: text.to_string(),
            namespace: "/test/deep_scan".to_string(),
        },
        delta: vec![10, 20, 30],
        previous_hash: [0u8; 32],
        current_hash: [9u8; 32],
        entropy_seeds: vec![],
        network_responses: vec![],
    }
}
