use raqim_core::aegis::AtomicTokenBucket;
use std::sync::Arc;

#[test]
fn test_atomic_token_bucket_never_underflow_under_massive_concurrency() {
    // 10 tokens capacity, 0 refill rate
    let bucket = Arc::new(AtomicTokenBucket::new(0, 10));
    let mut handles = Vec::new();

    // Spawn 100 concurrent threads competing for only 10 tokens
    for _ in 0..100 {
        let bucket_clone = bucket.clone();
        handles.push(std::thread::spawn(move || bucket_clone.check_and_consume()));
    }

    let mut successful_consumption = 0;
    for handle in handles {
        if handle.join().unwrap() {
            successful_consumption += 1;
        }
    }

    let final_token = bucket.tokens.load(std::sync::atomic::Ordering::SeqCst);

    // Asserts exactly 10 request successded
    assert_eq!(
        successful_consumption, 10,
        "CRIT-05 REGRESSION: Bucket permitted more than burst capacity!"
    );

    // Assets token must be equal exactly 0, never u64::MAX
    assert_eq!(
        final_token, 0,
        "CRIT-05 REGRESSION: Token bucket underflowed! Tokens = {}",
        final_token
    );
}
