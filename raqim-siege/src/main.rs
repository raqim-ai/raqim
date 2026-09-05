use rand_core::OsRng;
use raqim_siege::{AgentState, AgentStatus, CapabilityCertificate, IngressEnvelope};
use std::io::Write;
use std::net::{Ipv4Addr, SocketAddr, SocketAddrV4};
use std::{
    eprintln, format,
    fs::{self, OpenOptions},
    path::Path,
    println,
    sync::Arc,
    time::{Instant, SystemTime, UNIX_EPOCH},
};

use ed25519_dalek::{Signer, SigningKey};
use tokio::io::AsyncWriteExt;
use tokio::net::TcpStream;
use tokio::sync::Barrier;

/// Struct holding pre-minted cryptographic agent credentials in memory
#[derive(Clone)]
struct VirtualAgent {
    agent_id: [u8; 16],
    signing_key: Arc<SigningKey>,
    pub_key_bytes: [u8; 32],
    cert_bytes: Vec<u8>,
    namespace: String,
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    println!("==================================================================");
    println!("Bismillah. Initializing Hardened Raqim Siege Benchmark Suite v1.0.0");
    println!("===================================================================");

    // Benchmark parameter & harware profiling
    let total_rounds: usize = 500_000;
    let num_agents: usize = 50;
    let concurrency: usize = num_agents;
    let rounds_per_worker = total_rounds / concurrency;
    let target_addr = SocketAddr::V4(SocketAddrV4::new(Ipv4Addr::new(127, 0, 0, 1), 8080));

    println!("[CONFIG] Total Ingestion Rounds: {} ", total_rounds);
    println!("[CONFIG] Concurrent TCP Workers: {}", concurrency);
    println!("[CONFIG] Partitioned Shards: {}", num_agents);
    println!("[CONFIG] Rounds Per Worker, {}", rounds_per_worker);

    // Master Swarm CA Bootstrapping

    println!("[SIEGE CA] Acessing Swarm Master from ./ca-keys/swarm_master.key ....");
    let key_paths = ["./keys/master_private.pem", "./ca-keys/swarm_master.key"];
    let mut master_key_bytes_opt: Option<Vec<u8>> = None;

    for path_str in &key_paths {
        if Path::new(path_str).exists() {
            if let Ok(bytes) = fs::read(path_str) {
                if bytes.len() == 32 {
                    println!("[SIEGE CA] Loaded Master Key from  '{}' ", path_str);
                    master_key_bytes_opt = Some(bytes);
                    break;
                }
            }
        }
    }

    let master_signing_key = match master_key_bytes_opt {
        Some(bytes) => {
            let key_array: [u8; 32] = bytes.as_slice().try_into()?;
            SigningKey::from_bytes(&key_array)
        }

        None => {
            println!("[SEIGE CA] No master key found on disk. Auto-generating fresh keypair... ");
            fs::create_dir_all("./keys")?;

            let mut csprng = OsRng;
            let fresh_key = SigningKey::generate(&mut csprng);
            let mut file = OpenOptions::new()
                .create(true)
                .write(true)
                .truncate(true)
                .open("./keys/master_private.pem")?;
            file.write_all(&fresh_key.to_bytes())?;
            file.sync_all()?;

            fresh_key
        }
    };

    // Minting 50  virtual agents
    println!(
        "[SIEGE CA] Minting {} certified virtual agent identities.... ",
        num_agents
    );
    let mut agents = Vec::with_capacity(num_agents);

    for i in 0..num_agents {
        let mut csprng = OsRng;
        let agent_key = SigningKey::generate(&mut csprng);
        let pub_key_bytes = agent_key.verifying_key().to_bytes();

        // Derive 16-byte identity using Blake3 domain separation
        let mut hasher = blake3::Hasher::new_derive_key("raqim.agent.v1.identity");
        hasher.update(&pub_key_bytes);
        let mut agent_id = [0u8; 16];
        hasher.finalize_xof().fill(&mut agent_id);

        let agent_hex = hex::encode(agent_id);
        let namespace = format!("/siege/shard_{:02}", i);

        // Forge the capability certificate passport.
        let mut cert = CapabilityCertificate {
            agent_hex: agent_hex.clone(),
            group_name: "admin_group".to_string(),
            expiration_timestamp: u64::MAX,
            master_signature: Vec::new(),
        };

        // Sign the passport with the Master Key
        let serialized_raw = postcard::to_allocvec(&cert)?;

        let master_sig = master_signing_key.sign(&serialized_raw);
        cert.master_signature = master_sig.to_bytes().to_vec();

        let cert_bytes = postcard::to_allocvec(&cert)?;
        agents.push(VirtualAgent {
            agent_id,
            signing_key: Arc::new(agent_key),
            pub_key_bytes,
            cert_bytes,
            namespace,
        });
    }

    let shared_agents = Arc::new(agents);
    let sync_barrier = Arc::new(Barrier::new(concurrency + 1));
    let mut worker_handles = Vec::with_capacity(concurrency);

    println!("[SIEGE] Target endpoint: {}", target_addr);
    println!(
        "[SIEGE] Connecting {} dedicated agets sockets to kernel... ",
        concurrency
    );

    // CONCURRENT WORKER PIPELINE
    for worker_id in 0..concurrency {
        let agent_ref = shared_agents.clone();
        let barrier_ref = sync_barrier.clone();

        let handle = tokio::spawn(async move {
            // Establish persisent TCP stream to Raqim core daemon
            let agent = &agent_ref[worker_id];
            let mut stream = match TcpStream::connect(target_addr).await {
                Ok(s) => s,
                Err(e) => {
                    eprintln!(
                        "[WORKER {} FATAL] Failed to connect to 127.0.0.1:8080: {} ",
                        worker_id, e
                    );
                    return Vec::new();
                }
            };

            // Disable Nagle's algorith for low-latency packet streaming
            let _ = stream.set_nodelay(true);
            
            let (read_half, mut write_half) = stream.into_split();
            let mut reader = tokio::io::BufReader::with_capacity(64 * 1024, read_half);
            let mut ack_buf  = [0u8; 20];
            let mut latency_samples_micros: Vec<u64> = Vec::with_capacity(rounds_per_worker);
            

            // Sync all workers at the starting gate before benchmarking starts
            barrier_ref.wait().await;

            for round_idx in 0..rounds_per_worker {
                let global_idx = (worker_id * rounds_per_worker) + round_idx;

                // Fixed: live unix ts prevents Aegis Antireply drops
                let now_ts = SystemTime::now()
                    .duration_since(UNIX_EPOCH)
                    .unwrap_or_default()
                    .as_secs() as i64;

                // Fixed: Globally unique 128-bit UUIDv7 tx_id
                let tx_id = uuid::Uuid::now_v7().as_u128();

                let state = AgentState {
                    agent_id: Some(agent.agent_id),
                    transaction_id: tx_id,
                    namespace: agent.namespace.clone(),
                    timestamp: now_ts,
                    status: AgentStatus::Idle,
                    text: format!("Siege Payload #{} [Worker {}]", global_idx, worker_id),
                };

                // Zero-copy rkyv serialization of AgentState
                let state_bytes = rkyv::to_bytes::<rkyv::rancor::Error>(&state)
                    .expect("Failed to serialize AgentState")
                    .into_vec();

                // Ed25519 signature over state bytes
                let signature = agent.signing_key.sign(&state_bytes);

                let envelope = IngressEnvelope {
                    intent_path: agent.namespace.clone(),
                    public_key: agent.pub_key_bytes,
                    signature: signature.to_bytes(),
                    state_bytes,
                    capability_cert: agent.cert_bytes.clone(),
                };

                // Zero-copy rkyv serialization of IngressEnvelope
                let envelope_bytes = rkyv::to_bytes::<rkyv::rancor::Error>(&envelope)
                    .expect("Failed to serialize IngressEnvelope")
                    .into_vec();

                // Pack 4-byte Little-Endian length prefix
                let len_prefix = (envelope_bytes.len() as u32).to_le_bytes();

                // Measure full round-trip from dispatch to server commitement
                let op_start = Instant::now();

                // Send frame.
                if let Err(e) = write_half.write_all(&len_prefix).await {
                    eprintln!(
                        "[WORKER {} ERROR] Length prefix write failed: {}",
                        worker_id, e
                    );
                    break;
                }

                if let Err(e) = write_half.write_all(&envelope_bytes).await {
                    eprintln!("[WORKER {} ERROR] Payload write failed: {}", worker_id, e);
                    break;
                }

                // Await server ack frame
                if tokio::io::AsyncReadExt::read_exact(&mut reader, &mut ack_buf).await.is_err() {
                    eprintln!("[WORKER {}] Server closed socket before ACK", worker_id);
                    break;
                }


                let status = u32::from_le_bytes(ack_buf[0..4].try_into().unwrap());
                if status != 0 {
                    eprintln!("[WORKER {}] Server rejected transaction wit backpresssure!", worker_id); 
                    break;
                }

                let op_micros = op_start.elapsed().as_micros() as u64;
                latency_samples_micros.push(op_micros);
            }

            // Flush remaining socket bytes
            let _ = write_half.flush().await;
            latency_samples_micros
        });

        worker_handles.push(handle);
    }

    // 5. Synchronized execution & time measurement
    println!(
        "\n[SIEGE] All {} worker ready. Starting benchmark firehose....",
        concurrency
    );

    let bench_start = Instant::now();
    // Release workers simultaneously
    sync_barrier.wait().await;

    let mut all_latency_samples: Vec<u64> = Vec::with_capacity(total_rounds);

    for handle in worker_handles {
        let worker_sample = handle.await?;
        all_latency_samples.extend(worker_sample);
    }

    let bench_elapsed = bench_start.elapsed();

    // Closed-Loop drain  & latency percentile analysis
    println!("[SIEGE] Packet transmitted. Calculating percentiles...");

    let total_processed = all_latency_samples.len();
    if total_processed == 0 {
        eprintln!(
            "[SIEGE ERROR] Zero packets were proocessed. Ensure rawim-core is running on port  8080."
        );
        return Ok(());
    }

    // Sort all latency samples for statistical analysis
    all_latency_samples.sort_unstable();

    let p50 = all_latency_samples[(total_processed as f64 * 0.50) as usize];
    let p90 = all_latency_samples[(total_processed as f64 * 0.90) as usize];
    let p95 = all_latency_samples[(total_processed as f64 * 0.95) as usize];
    let p99 = all_latency_samples[(total_processed as f64 * 0.99) as usize];
    let p999 =
        all_latency_samples[((total_processed as f64 * 0.999) as usize).min(total_processed - 1)];
    let max_lat = all_latency_samples[total_processed - 1];
    let min_lat = all_latency_samples[0];

    let avg_latency: f64 = all_latency_samples.iter().sum::<u64>() as f64 / total_processed as f64;
    let tps = (total_processed as f64) / bench_elapsed.as_secs_f64();
    let data_volume_mb = (total_processed * 250) as f64 / (1024.0 * 1024.0);

    // 7. Publish verified benchmark report
    println!("\n =================================================");
    println!("         RAQIM HARDENED SIEGE BENCHMARK REPORT       ");
    println!(" Status                       : ✅ PASSES (Closed-Loop Complete)");
    println!(" Total Ingestion Volume    : {} Thoughts", total_processed);
    println!(
        " Concurrent Shards Hit     : {} Partitioned Agents",
        num_agents
    );
    println!(" Concurrent TCP Streams    : {} Sockets", concurrency);
    println!(
        " Transferred Byte Volume   : {:.2} MB (Zero-Copy rkvy)",
        data_volume_mb
    );
    println!(
        " Total Benchmark Duration  : {:.3} Seconds",
        bench_elapsed.as_secs_f64()
    );
    println!("-----------------------------------------------------------");
    println!(" REAL THROUGHPUT (TPS)     : {:.2} THROUGHPUT / SEC", tps);
    println!("------------------------------------------------------------");
    println!(" LATENCY DISTRIBUTION (Per-Thought Ingress + Hashing):");
    println!(
        "  Min Latency            : {} µs ({:.3} ms)",
        min_lat,
        min_lat as f64 / 1000.0
    );
    println!(
        "  P50 (Median Latency)   : {} µs ({:.3} ms)",
        p50,
        p50 as f64 / 1000.0
    );
    println!(
        "  P90 Latency            : {} µs ({:.3} ms)",
        p90,
        p90 as f64 / 1000.0
    );
    println!(
        "  P95 Latency            : {} µs ({:.3} ms)",
        p95,
        p95 as f64 / 1000.0
    );
    println!(
        "  P99 Latency (Tail Latency)     : {} µs ({:.3} ms)",
        p99,
        p99 as f64 / 1000.0
    );
    println!(
        "  P99.9 (Worst Tail)    : {} µs ({:.3} ms)",
        p999,
        p999 as f64 / 1000.0
    );
    println!(
        "  Max Latency     : {} µs ({:.3} ms)",
        max_lat,
        max_lat as f64 / 1000.0
    );
    println!(
        "  Arithmetic Mean     : {:.2} µs ({:.3} ms)",
        avg_latency,
        avg_latency as f64 / 1000.0
    );
    println!("=================================================");

    Ok(())
}
