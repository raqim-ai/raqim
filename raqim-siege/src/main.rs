// ==============================================================================
// RAQIM v0.2.0 HARDENED SIEGE BENCHMARK SUITE
// ==============================================================================
// Measures TRUE closed-loop durability and gateway performance against `raqim-core`:
//   1. `tcp` mode    : Binary zero-copy `rkyv` `ExecutionRecord` -> Ed25519 ->
//                      PolicyEngine -> synchronous `fdatasync` WAL ACK -> Merkle.
//   2. `egress` mode : Zero-Secret HTTP Egress Gateway (`/v1/egress/...`) ->
//                      `rqm_v1` EdDSA token -> 4 Concentric Locks -> WAL / Replay.
// ==============================================================================

use clap::{Parser, Subcommand};
use ed25519_dalek::{Signer, SigningKey};
use rand_core::OsRng;
use raqim_core::policy::CapabilityCertificate;
use raqim_core::token::mint_workload_token;
use raqim_core::{ExecutionRecord, ExecutionStage, IngressEnvelope};
use std::fs;
use std::net::{Ipv4Addr, SocketAddr, SocketAddrV4};
use std::path::Path;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;
use std::time::{Instant, SystemTime, UNIX_EPOCH};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpStream;
use tokio::sync::Barrier;

#[derive(Parser)]
#[command(
    name = "raqim-siege",
    about = "Raqim v0.2.0 Closed-Loop Durability & Egress Gateway Benchmark Suite",
    version = "0.2.0"
)]
struct Cli {
    /// Path to the Swarm Master Ed25519 signing key
    #[arg(long, default_value = "./ca-keys/swarm_master.key", global = true)]
    master_key: String,

    #[command(subcommand)]
    mode: BenchMode,
}

#[derive(Subcommand)]
enum BenchMode {
    /// Benchmark Binary TCP Ingress (:8080) with synchronous NVMe fdatasync ACKs
    Tcp {
        #[arg(long, default_value_t = 8080)]
        port: u16,
        /// Total transactions to dispatch across all workers
        #[arg(short, long, default_value_t = 25_000)]
        rounds: usize,
        /// Number of concurrent virtual agents / persistent TCP streams
        #[arg(short, long, default_value_t = 50)]
        workers: usize,
        /// Security group from policy.toml
        #[arg(short, long, default_value = "admin_group")]
        group: String,
    },
    /// Benchmark HTTP Zero-Secret Egress Gateway (:8081/v1/egress/{connector}/{path})
    Egress {
        #[arg(long, default_value = "http://127.0.0.1:8081")]
        gateway_url: String,
        #[arg(long, default_value = "paystack")]
        connector: String,
        #[arg(long, default_value = "/transfer")]
        path: String,
        #[arg(short, long, default_value_t = 2_000)]
        rounds: usize,
        #[arg(short, long, default_value_t = 25)]
        workers: usize,
        #[arg(short, long, default_value = "admin_group")]
        group: String,
        /// Egress replay header mode: "replay" (measures gateway $0 cache path) or "live"
        #[arg(long, default_value = "replay")]
        raqim_mode: String,
        /// JSON request body to send on each call
        #[arg(long, default_value = r#"{"amount":1000,"currency":"NGN"}"#)]
        body: String,
    },
}

#[derive(Clone)]
struct VirtualTcpAgent {
    agent_id: [u8; 16],
    signing_key: Arc<SigningKey>,
    pub_key_bytes: [u8; 32],
    cert_bytes: Vec<u8>,
    namespace: String,
}

fn load_master_key(path_str: &str) -> Result<SigningKey, Box<dyn std::error::Error>> {
    if !Path::new(path_str).exists() {
        return Err(format!(
            "Master key not found at '{}'. Boot raqim-core or run `raqim-cli keys forge` first.",
            path_str
        )
        .into());
    }
    let bytes = fs::read(path_str)?;
    let arr: [u8; 32] = bytes
        .as_slice()
        .try_into()
        .map_err(|_| "Master key file must be exactly 32 bytes")?;
    Ok(SigningKey::from_bytes(&arr))
}

fn print_report(
    label: &str,
    total_processed: usize,
    workers: usize,
    elapsed_secs: f64,
    samples_us: &mut Vec<u64>,
    extra_notes: &str,
) {
    if total_processed == 0 || samples_us.is_empty() {
        eprintln!("[SIEGE ERROR] Zero successful requests completed. Verify daemon is running and policy allows traffic.");
        return;
    }

    samples_us.sort_unstable();
    let n = samples_us.len();
    let min_lat = samples_us[0];
    let p50 = samples_us[(n as f64 * 0.50) as usize];
    let p90 = samples_us[(n as f64 * 0.90) as usize];
    let p95 = samples_us[(n as f64 * 0.95) as usize];
    let p99 = samples_us[((n as f64 * 0.99) as usize).min(n - 1)];
    let p999 = samples_us[((n as f64 * 0.999) as usize).min(n - 1)];
    let max_lat = samples_us[n - 1];
    let avg_lat = samples_us.iter().sum::<u64>() as f64 / n as f64;
    let tps = total_processed as f64 / elapsed_secs;

    println!("\n==================================================================");
    println!("  RAQIM v0.2.0 SIEGE BENCHMARK REPORT — {}", label);
    println!("==================================================================");
    println!("  Completed Requests : {}", total_processed);
    println!("  Concurrent Agents  : {}", workers);
    println!("  Total Duration     : {:.3} s", elapsed_secs);
    if !extra_notes.is_empty() {
        println!("  Breakdown          : {}", extra_notes);
    }
    println!("------------------------------------------------------------------");
    println!("  SUSTAINED TPS      : {:.2} req/sec (True Closed-Loop ACK)", tps);
    println!("------------------------------------------------------------------");
    println!("  Min Latency        : {:>8} µs ({:.3} ms)", min_lat, min_lat as f64 / 1000.0);
    println!("  P50 (Median)       : {:>8} µs ({:.3} ms)", p50, p50 as f64 / 1000.0);
    println!("  P90                : {:>8} µs ({:.3} ms)", p90, p90 as f64 / 1000.0);
    println!("  P95                : {:>8} µs ({:.3} ms)", p95, p95 as f64 / 1000.0);
    println!("  P99 (Tail)         : {:>8} µs ({:.3} ms)", p99, p99 as f64 / 1000.0);
    println!("  P99.9              : {:>8} µs ({:.3} ms)", p999, p999 as f64 / 1000.0);
    println!("  Max Latency        : {:>8} µs ({:.3} ms)", max_lat, max_lat as f64 / 1000.0);
    println!("  Mean Latency       : {:>8.1} µs ({:.3} ms)", avg_lat, avg_lat / 1000.0);
    println!("==================================================================");
}

async fn run_tcp_benchmark(
    master_signing_key: SigningKey,
    port: u16,
    total_rounds: usize,
    workers: usize,
    group: String,
) -> Result<(), Box<dyn std::error::Error>> {
    let rounds_per_worker = (total_rounds / workers).max(1);
    let target_addr = SocketAddr::V4(SocketAddrV4::new(Ipv4Addr::new(127, 0, 0, 1), port));

    println!("==================================================================");
    println!("Bismillah. Raqim v0.2.0 Synchronous-fdatasync TCP Siege");
    println!("Target: {} | Agents: {} | Rounds/Agent: {} | Total: {}",
        target_addr, workers, rounds_per_worker, rounds_per_worker * workers);
    println!("==================================================================");

    let exp = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs()
        + 86_400;

    let mut agents = Vec::with_capacity(workers);
    for i in 0..workers {
        let agent_key = SigningKey::generate(&mut OsRng);
        let pub_key_bytes = agent_key.verifying_key().to_bytes();

        let mut hasher = blake3::Hasher::new_derive_key("raqim.agent.v1.identity");
        hasher.update(&pub_key_bytes);
        let mut agent_id = [0u8; 16];
        hasher.finalize_xof().fill(&mut agent_id);
        let agent_hex = hex::encode(agent_id);

        let mut cert = CapabilityCertificate {
            agent_hex,
            group_name: group.clone(),
            expiration_timestamp: exp,
            master_signature: Vec::new(),
        };
        let unsigned = postcard::to_allocvec(&cert)?;
        cert.master_signature = master_signing_key.sign(&unsigned).to_bytes().to_vec();
        let cert_bytes = postcard::to_allocvec(&cert)?;

        agents.push(VirtualTcpAgent {
            agent_id,
            signing_key: Arc::new(agent_key),
            pub_key_bytes,
            cert_bytes,
            namespace: format!("/siege/shard_{:02}", i),
        });
    }

    let shared_agents = Arc::new(agents);
    let sync_barrier = Arc::new(Barrier::new(workers + 1));
    let rejected_count = Arc::new(AtomicUsize::new(0));
    let mut handles = Vec::with_capacity(workers);

    for worker_id in 0..workers {
        let agent_ref = shared_agents.clone();
        let barrier_ref = sync_barrier.clone();
        let rejected_ref = rejected_count.clone();

        handles.push(tokio::spawn(async move {
            let agent = &agent_ref[worker_id];
            let mut stream = match TcpStream::connect(target_addr).await {
                Ok(s) => s,
                Err(e) => {
                    eprintln!("[WORKER {}] Connect failed: {}", worker_id, e);
                    barrier_ref.wait().await;
                    return Vec::new();
                }
            };
            let _ = stream.set_nodelay(true);

            let (read_half, mut write_half) = stream.into_split();
            let mut reader = tokio::io::BufReader::with_capacity(64 * 1024, read_half);
            let mut ack_buf = [0u8; 20];
            let mut latencies = Vec::with_capacity(rounds_per_worker);

            barrier_ref.wait().await;

            for round_idx in 0..rounds_per_worker {
                let global_idx = (worker_id * rounds_per_worker) + round_idx;
                let record = ExecutionRecord::new(
                    agent.agent_id,
                    round_idx as u64,
                    agent.namespace.clone(),
                    ExecutionStage::Outcome,
                    [0u8; 32],
                    format!("Siege v0.2.0 Record #{} [Worker {}]", global_idx, worker_id)
                        .into_bytes(),
                );

                let record_bytes = rkyv::to_bytes::<rkyv::rancor::Error>(&record)
                    .expect("ExecutionRecord serialization failed")
                    .into_vec();
                let signature = agent.signing_key.sign(&record_bytes).to_bytes();

                let envelope = IngressEnvelope {
                    public_key: agent.pub_key_bytes,
                    signature,
                    record_bytes,
                    capability_cert: agent.cert_bytes.clone(),
                };

                let envelope_bytes = rkyv::to_bytes::<rkyv::rancor::Error>(&envelope)
                    .expect("IngressEnvelope serialization failed")
                    .into_vec();
                let len_prefix = (envelope_bytes.len() as u32).to_le_bytes();

                let op_start = Instant::now();
                if write_half.write_all(&len_prefix).await.is_err()
                    || write_half.write_all(&envelope_bytes).await.is_err()
                {
                    rejected_ref.fetch_add(1, Ordering::Relaxed);
                    break;
                }

                if reader.read_exact(&mut ack_buf).await.is_err() {
                    rejected_ref.fetch_add(1, Ordering::Relaxed);
                    break;
                }

                let status = u32::from_le_bytes(ack_buf[0..4].try_into().unwrap());
                if status != 0 {
                    rejected_ref.fetch_add(1, Ordering::Relaxed);
                    break;
                }

                latencies.push(op_start.elapsed().as_micros() as u64);
            }

            latencies
        }));
    }

    let bench_start = Instant::now();
    sync_barrier.wait().await;

    let mut all_samples = Vec::with_capacity(rounds_per_worker * workers);
    for h in handles {
        all_samples.extend(h.await?);
    }
    let elapsed = bench_start.elapsed().as_secs_f64();
    let rejected = rejected_count.load(Ordering::Relaxed);

    print_report(
        "BINARY TCP INGRESS (TRUE fdatasync ACK)",
        all_samples.len(),
        workers,
        elapsed,
        &mut all_samples,
        &format!("Rejected/Dropped Workers: {}", rejected),
    );
    Ok(())
}

#[allow(clippy::too_many_arguments)]
async fn run_egress_benchmark(
    master_signing_key: SigningKey,
    gateway_url: String,
    connector: String,
    path: String,
    total_rounds: usize,
    workers: usize,
    group: String,
    raqim_mode: String,
    body: String,
) -> Result<(), Box<dyn std::error::Error>> {
    let rounds_per_worker = (total_rounds / workers).max(1);
    let clean_path = path.trim_start_matches('/');
    let target_url = format!(
        "{}/v1/egress/{}/{}",
        gateway_url.trim_end_matches('/'),
        connector,
        clean_path
    );

    println!("==================================================================");
    println!("Bismillah. Raqim v0.2.0 Zero-Secret Egress Gateway Siege");
    println!("URL: {} | Mode: {} | Agents: {} | Total: {}",
        target_url, raqim_mode, workers, rounds_per_worker * workers);
    println!("==================================================================");

    let mut tokens = Vec::with_capacity(workers);
    for _ in 0..workers {
        let agent_key = SigningKey::generate(&mut OsRng);
        let pub_bytes = agent_key.verifying_key().to_bytes();
        let mut hasher = blake3::Hasher::new_derive_key("raqim.agent.v1.identity");
        hasher.update(&pub_bytes);
        let mut agent_id = [0u8; 16];
        hasher.finalize_xof().fill(&mut agent_id);
        let agent_hex = hex::encode(agent_id);

        let tok = mint_workload_token(&master_signing_key, &agent_hex, &group, 3600)?;
        tokens.push(tok);
    }

    let client = reqwest::Client::builder()
        .no_proxy()
        .pool_max_idle_per_host(workers)
        .build()?;

    // If benchmarking replay mode, prime step 0 once per worker so the WAL cache is warm
    if raqim_mode.eq_ignore_ascii_case("replay") {
        println!("[SIEGE] Priming 1 live call per virtual agent to warm WAL replay cache...");
        for tok in &tokens {
            let _ = client
                .post(&target_url)
                .header("Authorization", format!("Bearer {}", tok))
                .header("Content-Type", "application/json")
                .header("X-Raqim-Step", "0")
                .header("X-Raqim-Mode", "record")
                .body(body.clone())
                .send()
                .await;
        }
    }

    let ok_200 = Arc::new(AtomicUsize::new(0));
    let rate_429 = Arc::new(AtomicUsize::new(0));
    let denied_403 = Arc::new(AtomicUsize::new(0));
    let other_err = Arc::new(AtomicUsize::new(0));
    let barrier = Arc::new(Barrier::new(workers + 1));
    let mut handles = Vec::with_capacity(workers);

    for tok in tokens {
        let http = client.clone();
        let url = target_url.clone();
        let mode_hdr = raqim_mode.clone();
        let payload = body.clone();
        let b_ref = barrier.clone();
        let c200 = ok_200.clone();
        let c429 = rate_429.clone();
        let c403 = denied_403.clone();
        let cerr = other_err.clone();

        handles.push(tokio::spawn(async move {
            let mut latencies = Vec::with_capacity(rounds_per_worker);
            b_ref.wait().await;

            for _ in 0..rounds_per_worker {
                let t0 = Instant::now();
                let resp = http
                    .post(&url)
                    .header("Authorization", format!("Bearer {}", tok))
                    .header("Content-Type", "application/json")
                    .header("X-Raqim-Step", "0")
                    .header("X-Raqim-Mode", &mode_hdr)
                    .body(payload.clone())
                    .send()
                    .await;

                match resp {
                    Ok(r) => match r.status().as_u16() {
                        200..=299 => {
                            c200.fetch_add(1, Ordering::Relaxed);
                            latencies.push(t0.elapsed().as_micros() as u64);
                        }
                        429 => {
                            c429.fetch_add(1, Ordering::Relaxed);
                        }
                        403 => {
                            c403.fetch_add(1, Ordering::Relaxed);
                            break;
                        }
                        _ => {
                            cerr.fetch_add(1, Ordering::Relaxed);
                        }
                    },
                    Err(_) => {
                        cerr.fetch_add(1, Ordering::Relaxed);
                    }
                }
            }
            latencies
        }));
    }

    let bench_start = Instant::now();
    barrier.wait().await;

    let mut all_samples = Vec::with_capacity(rounds_per_worker * workers);
    for h in handles {
        all_samples.extend(h.await?);
    }
    let elapsed = bench_start.elapsed().as_secs_f64();

    let notes = format!(
        "2xx: {} | 429 RateLimited: {} | 403 Denied: {} | Other Err: {}",
        ok_200.load(Ordering::Relaxed),
        rate_429.load(Ordering::Relaxed),
        denied_403.load(Ordering::Relaxed),
        other_err.load(Ordering::Relaxed)
    );

    print_report(
        &format!("HTTP EGRESS GATEWAY ({})", raqim_mode.to_uppercase()),
        all_samples.len(),
        workers,
        elapsed,
        &mut all_samples,
        ¬es,
    );
    Ok(())
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let cli = Cli::parse();
    let master_signing_key = load_master_key(&cli.master_key)?;

    match cli.mode {
        BenchMode::Tcp {
            port,
            rounds,
            workers,
            group,
        } => run_tcp_benchmark(master_signing_key, port, rounds, workers, group).await?,
        BenchMode::Egress {
            gateway_url,
            connector,
            path,
            rounds,
            workers,
            group,
            raqim_mode,
            body,
        } => {
            run_egress_benchmark(
                master_signing_key,
                gateway_url,
                connector,
                path,
                rounds,
                workers,
                group,
                raqim_mode,
                body,
            )
            .await?
        }
    }

    Ok(())
}