use raqim_core::wal::{WalEngine, WalError};
use raqim_core::{ExecutionRecord, ExecutionStage};
use std::fs::OpenOptions;
use std::io::{Seek, SeekFrom, Write};
use std::path::Path;

#[tokio::test]
async fn test_wal_synchronous_group_commit_and_mid_file_vs_tail_corruption() {
    let wal_path = "test_phase0_durability.wal";
    let _ = std::fs::remove_file(wal_path);

    // 1. Write 2 distinct batches and verify synchronous durability
    {
        let (wal, handle) = WalEngine::start(wal_path.to_string()).await;
        let rec1 = ExecutionRecord::new(
            [1u8; 16],
            1,
            b"batch_1_payload".to_vec(),
            ExecutionStage::Outcome,
            "/finance/screen".to_string(),
            [0xAA; 32],
        );
        wal.append(rec1).await.expect("Batch 1 append failed");

        let rec2 = ExecutionRecord::new(
            [1u8; 16],
            2,
            b"batch_2_payload".to_vec(),
            ExecutionStage::Outcome,
            "/finance/screen".to_string(),
            [0xBB; 32],
        );
        wal.append(rec2).await.expect("Batch 2 append failed");

        drop(wal);
        let _ = handle.await;
    }

    let clean_len = std::fs::metadata(wal_path).unwrap().len();
    assert!(clean_len > 32, "WAL must contain committed frames on disk");

    // 2. MID-FILE CORRUPTION TEST: Flip 1 byte in Batch 1 (offset 16) while Batch 2 follows it.
    // Must refuse to silently truncate Batch 2!
    {
        let mut f = OpenOptions::new()
            .read(true)
            .write(true)
            .open(wal_path)
            .unwrap();
        f.seek(SeekFrom::Start(16)).unwrap();
        f.write_all(&[0xFF]).unwrap();
        f.sync_all().unwrap();
    }

    let mid_res = WalEngine::recover_and_truncate_torn_frames(wal_path);
    assert!(
        matches!(mid_res, Err(WalError::MidFileCorruption(_))),
        "Mid-file corruption MUST return Err(MidFileCorruption), not silently truncate!"
    );

    // Restore that byte back or test tail torn-write truncation on a fresh WAL
    let _ = std::fs::remove_file(wal_path);
    {
        let (wal, handle) = WalEngine::start(wal_path.to_string()).await;
        let rec1 = ExecutionRecord::new(
            [2u8; 16],
            1,
            b"valid_batch".to_vec(),
            ExecutionStage::Outcome,
            "/finance/screen".to_string(),
            [0xCC; 32],
        );
        wal.append(rec1).await.unwrap();
        drop(wal);
        let _ = handle.await;
    }

    let valid_len = std::fs::metadata(wal_path).unwrap().len();

    // 3. TAIL TORN-WRITE TEST: Append 5 half-written bytes at EOF (simulating power loss)
    {
        let mut f = OpenOptions::new().append(true).open(wal_path).unwrap();
        f.write_all(&[100, 0, 0, 0, 0xAA]).unwrap();
        f.sync_all().unwrap();
    }

    let (truncated_len, index) = WalEngine::recover_and_truncate_torn_frames(wal_path)
        .expect("Tail torn write should truncate cleanly");
    assert_eq!(
        truncated_len, valid_len,
        "Tail torn write must truncate back to exact valid_len"
    );
    assert_eq!(
        index.len(),
        1,
        "Valid batch before torn tail must be preserved"
    );

    if Path::new(wal_path).exists() {
        let _ = std::fs::remove_file(wal_path);
    }
}
