use blake3::Hasher;
use clap::{Parser, Subcommand};
use ed25519_dalek::SigningKey;
use rand::rngs::OsRng;
use reqwest::Client;
use serde::{Deserialize, Serialize};
use serde_json::json;
use std::path::Path;
use std::time::{Duration, SystemTime, UNIX_EPOCH};
use std::{fs, println};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
#[cfg(unix)]
use tokio::net::UnixStream;

#[derive(Clone, Debug, Serialize, Deserialize, rkyv::Archive, rkyv::Serialize)]
pub struct QuarantineRecord {
    pub agent_hex: String,
    pub violation_type: String,
    pub attemped_path: String,
    pub payload_preview: String,
    pub timestamp: u64,
}

#[derive(Parser)]
#[command(
    name = "raqim",
    about = "Raqim v0.2.0 Sovereign Execution-Integrity & Gateway Operator CLI ",
    version = "0.2.0"
)]
struct Cli {
    /// Path to Operator Admin Unixx Domain Socket (mode 0600)
    #[arg(long, default_value = "./vault/admin.sock", global = true)]
    admin_socket: String,

    /// Path to the Operator Admin Bearer Secret file
    #[arg(long, default_value = "http://127.0.0.1:8081", global = true)]
    data_url: String,

    #[command(subcommand)]
    command: Commands,
}

#[derive(Subcommand)]
enum Commands {
    /// Offline Cryptographic Identity & Workload Token Provisioning
    Keys {
        #[command(subcommand)]
        action: KeyAction,
    },

    /// Pre-Execution PolicyEngine & Quarantine management
    Policy {
        #[command(subcommand)]
        action: PolicyAction,
    },

    /// System Health, Key PErmission Audit & In-Doubt crash Reconciliation
    Doctor {

        /// Only display unresolved in-doubt intents 
        #[arg(long)]
        in_doubt: bool,

    },

    /// Live Terminal Stream of Intents, outcomes, and policy Denials 
    Tail {
        /// Number of historical records to fetch initially 
        #[arg(short, long, default_value_t = 25)]
        limit: usize, 
    }

    /// Execute NDPA / GDPR In-place PII Erasure on a Transaction ID 
    Redact {
        
        tx_id: String, 
    },

    /// Trigger manual 2PC WAL Rotation & ColdStore Compaction 
    Compact, 

    /// Export an Offline-Verifiable Cryptographic Evidence bundle for an agent 
    Export {
        /// 32-char hex Agent ID 
        #[arg(short, long)]

        /// Target output directory for the evidence bundle 
        #[arg(short, long)]
        out: Option<String> 
    },
}

#[derive(Subcommand)]
enum KeyAction {
    /// Offline forge of agent Ed25519 keys (.pem), certificates (.cert) and gateway tokens (.token)
    Forge {
        #[arg(short, long)]
        name: String,

        #[arg(short, long)]
        group: String,

        #[arg(short, long, default_value_t = 1)]
        count: u32,

        #[arg(long, default_value_t = 30)]
        ttl_days: u64, 

        #[arg(short, long, default_value = "./ca-keys")]
        out_dir: String,
    },
}

#[derive(Subcommand)]
enum PolicyAction {
    List,

    Lift {agent_id: String}, 

    Metrics 
}

/// Sends an uthenticated HTTP/1.1 request over the local Unix Domain Socket 
#[cfg(unix)]
async fn send_admin_uds_request(socket_path: &str, admin_secret: &str, method: &str, path: &str, json_body: Option<&Value>) -> Result<(u16, String), anyhow::Error> {

    let mut stream = UnixStream::connect(socket_path).await.map_err(|e| {
        anyhow::anyhow!("Failed to connect to Admin Unix Socket '{}' ({}). Is raqim-core running?", socket_path, e)
    } )?;

    let body_bytes = match json_body {
        Some(val) => serde_json::to_vec(val)?, 
        None => Vec::new()
    };

    let req_header = format!( "{} {} HTTP/1.1\r\nHost: localhost\r\nAuthorization: Bearer {}\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n", method, path, admin_secret.trim(), body_bytes.len() );

    stream.write_all(req_header.as_bytes()).await?; 
    if !body_bytes.is_empty() {
        stream.write_all(&body_bytes).await?;
    }
    stream.flush().await?;

    let mut raw_resp = Vec::new(); 
    stream.read_to_end(&mut raw_resp).await?; 
    let resp_str = String::from_utf8_lossy(&raw_resp).to_string();

    let (headers_part, body_part) = resp_str.split_once("\r\n\r\n").ok_or_else(|| anyhow::anyhow!("Malformed HTTP response from admin socket"))?;
    
    let status_line = headers_part.lines().next().unwrap_or("");
    let status_code: u16 = status_line.split_whitespace().nth(1).and_then(|s| s.parse().ok() ).unwrap_or(500);

    // Handle chunked transfer encoding if present, otherwise return raw body 
    let clean_body = if headers_part.to_ascii_lowercase().contains("transfer-encoding: chunked") {
        decoded_chunked_body(body_part)
    } else {
        body_part.to_string()
    }

    Ok((status_code, clean_body))
}

fn decode_chunked_body(raw: &str) -> String {
    let mut out = String::new();
    let mut rest = raw;
    while let Some((len_hex, after_len)) = rest.split_once("\r\n") {
        let Ok(chunk_len) = usize::from_str_radix(len_hex.trim(), 16) else {
            break;
        };
        if chunk_len == 0 {
            break;
        }
        if after_len.len() >= chunk_len {
            out.push_str(&after_len[..chunk_len]);
            rest = &after_len[chunk_len..];
            if rest.starts_with("\r\n") {
                rest = &rest[2..];
            }
        } else {
            break;
        }
    }
    if out.is_empty() {
        raw.to_string()
    } else {
        out
    }
}

fn load_admin_secret(secret_path: &str) -> Result<String, anyhow::Error> {
    if let Ok(env_val) = std::env::var("RAQIM_ADMIN_SECRET") {
        if !env_val.trim().is_empty() {
            return Ok(env_val.trim().to_string());
        }
    }
    fs::read_to_string(secret_path)
        .map(|s| s.trim().to_string())
        .map_err(|e| {
            anyhow::anyhow!(
                "Could not read admin secret at '{}' ({}). Set RAQIM_ADMIN_SECRET or start raqim-core first.",
                secret_path,
                e
            )
        })
}

#[derive(Subcommand)]
enum ClusterAction {
    /// Polls live node viitals, buffer loads, and WAL status
    Info,
    /// Inspect allocated Loro CRDT memory shards and active timelines
    Topology,
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let cli = Cli::parse();

    match &cli.command {

        // =====================================================================
        // 1. OFFLINE CRYPTOGRAPHIC KEY & WORKLOAD TOKEN FORGE
        // =====================================================================
        Commands::Keys {
            action:
                KeyAction::Forge {
                    name,
                    group,
                    count,
                    ttl_days,
                    out_dir,
                },
        } => {
            println!("==================================================================");
            println!("Bismillah. Raqim Offline Cryptographic Identity Forge");
            println!("Target Group: [{}] | Count: [{}] | TTL: [{} days]", group, count, ttl_days);
            println!("==================================================================");

            let workspace = Path::new(out_dir);
            fs::create_dir_all(workspace)?;

            // Load or initialize the offline Master Signing Key (`swarm_master.key`)
            let master_key_path = workspace.join("swarm_master.key");
            let master_signing_key = if master_key_path.exists() {
                let bytes = fs::read(&master_key_path)?;
                let arr: [u8; 32] = bytes
                    .as_slice()
                    .try_into()
                    .map_err(|_| "swarm_master.key must be 32 bytes")?;
                SigningKey::from_bytes(&arr)
            } else {
                println!("[FORGE] Initializing new Master Root Key at {:?}", master_key_path);
                let key = SigningKey::generate(&mut OsRng);
                fs::write(&master_key_path, key.to_bytes())?;
                #[cfg(unix)]
                {
                    use std::os::unix::fs::PermissionsExt;
                    fs::set_permissions(&master_key_path, fs::Permissions::from_mode(0o600))?;
                }
                key
            };

            let ttl_seconds = ttl_days.saturating_mul(86_400);
            let expiration = SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap_or_default()
                .as_secs()
                .saturating_add(ttl_seconds);

            for i in 1..=*count {
                let alias = if *count > 1 {
                    format!("{}_{:02}", name, i)
                } else {
                    name.clone()
                };

                let agent_signing_key = SigningKey::generate(&mut OsRng);
                let pub_key_bytes = agent_signing_key.verifying_key().to_bytes();

                let mut hasher = Hasher::new_derive_key("raqim.agent.v1.identity");
                hasher.update(&pub_key_bytes);
                let mut derived_16 = [0u8; 16];
                hasher.finalize_xof().fill(&mut derived_16);
                let agent_hex = hex::encode(derived_16);

                // 1. Sign binary CapabilityCertificate (for SDK / TCP)
                let mut cert = CapabilityCertificate {
                    agent_hex: agent_hex.clone(),
                    group_name: group.clone(),
                    expiration_timestamp: expiration,
                    master_signature: Vec::new(),
                };
                let unsigned_bytes = postcard::to_allocvec(&cert)?;
                cert.master_signature = master_signing_key.sign(&unsigned_bytes).to_bytes().to_vec();
                let cert_bytes = postcard::to_allocvec(&cert)?;

                // 2. Mint `rqm_v1` Ed25519 Workload Token (for Zero-Secret HTTP Egress Gateway)
                let workload_token =
                    mint_workload_token(&master_signing_key, &agent_hex, group, ttl_seconds)?;

                let pem_path = workspace.join(format!("{}.pem", alias));
                let cert_path = workspace.join(format!("{}.cert", alias));
                let token_path = workspace.join(format!("{}.token", alias));

                fs::write(&pem_path, agent_signing_key.to_bytes())?;
                fs::write(&cert_path, &cert_bytes)?;
                fs::write(&token_path, &workload_token)?;

                #[cfg(unix)]
                {
                    use std::os::unix::fs::PermissionsExt;
                    fs::set_permissions(&pem_path, fs::Permissions::from_mode(0o600))?;
                    fs::set_permissions(&token_path, fs::Permissions::from_mode(0o600))?;
                }

                println!("  [OK] Agent '{}' -> ID: {}", alias, agent_hex);
                println!("       ├─ SDK Private Key : {}", pem_path.display());
                println!("       ├─ SDK Certificate : {}", cert_path.display());
                println!("       └─ Gateway Token   : {} ({}...)", token_path.display(), &workload_token[..24]);
            }
        }

          // ======================================================================
    // 2. DOCTOR: PERMISSION AUDIT, KERNEL METRICS & IN-DOUBT RECONCILIATION
    // ======================================================================
    Commands::Doctor { in_doubt } => {
            let secret = load_admin_secret(&cli.admin_secret_path)?;

            if !*in_doubt {
                println!("==================================================================");
                println!("Raqim v0.2.0 System Doctor & Security Posture Check");
                println!("==================================================================");

                #[cfg(unix)]
                {
                    use std::os::unix::fs::PermissionsExt;
                    for path_str in ["./ca-keys/swarm_master.key", &cli.admin_secret_path, &cli.admin_socket] {
                        if let Ok(meta) = fs::metadata(path_str) {
                            let mode = meta.permissions().mode() & 0o777;
                            if mode & 0o077 == 0 {
                                println!("  [PASS] Permission check '{}' -> {:04o}", path_str, mode);
                            } else {
                                println!("  [WARN] Insecure permissions on '{}' -> {:04o} (Expected 0600)", path_str, mode);
                            }
                        }
                    }
                }

                let (status, info_body) =
                    send_admin_uds_request(&cli.admin_socket, &secret, "GET", "/v1/admin/cluster/info", None).await?;
                if status == 200 {
                    let info: Value = serde_json::from_str(&info_body)?;
                    println!("  [PASS] Admin Socket Connected ({})", cli.admin_socket);
                    println!("         ├─ Highest TxID   : {}", info["highest_tx_id"].as_str().unwrap_or("0x0"));
                    println!("         ├─ Active WAL     : {:.2} MB ({} bytes)", info["wal_size_mb"].as_f64().unwrap_or(0.0), info["wal_bytes"]);
                    println!("         └─ ColdStore Recs : {}", info["cold_records"]);
                } else {
                    eprintln!("  [FAIL] Admin Socket returned HTTP {}: {}", status, info_body);
                }
            }

            let (status, doubt_body) =
                send_admin_uds_request(&cli.admin_socket, &secret, "GET", "/v1/admin/in_doubt", None).await?;
            if status == 200 {
                let intents: Vec<ExecutionRecord> = serde_json::from_str(&doubt_body)?;
                if intents.is_empty() {
                    println!("  [PASS] In-Doubt Intents: 0 (All write-ahead intents resolved cleanly).");
                } else {
                    println!("\n  [ALERT] {} UNRESOLVED IN-DOUBT INTENT(S) DETECTED:", intents.len());
                    for rec in intents {
                        println!(
                            "    -> Intent TxID / Idempotency-Key: {:032x} | Agent: {} | Step: {} | Path: {}",
                            rec.tx_id,
                            hex::encode(rec.agent_id),
                            rec.step_ordinal,
                            rec.namespace
                        );
                    }
                }
            }
        }

        // =====================================================================
        // 3. POLICY & QUARANTINE MANAGEMENT
        // =====================================================================
        Commands::Policy { action } => {
            let secret = load_admin_secret(&cli.admin_secret_path)?;
            match action {
                PolicyAction::List => {
                    let (status, body) = send_admin_uds_request(
                        &cli.admin_socket,
                        &secret,
                        "GET",
                        "/v1/policy/quarantine_list",
                        None,
                    )
                    .await?;
                    if status == 200 {
                        let records: Vec<QuarantineRecord> = serde_json::from_str(&body)?;
                        println!("Active Quarantine Blocklist ({} Isolated):", records.len());
                        for r in records {
                            println!(
                                "  -> Agent: {} | Violation: {} | Path: {} | Detail: {}",
                                r.agent_hex, r.violation_type, r.attemped_path, r.payload_preview
                            );
                        }
                    } else {
                        eprintln!("Failed to fetch quarantine list (HTTP {}): {}", status, body);
                    }
                }
                PolicyAction::Lift { agent_id } => {
                    let payload = json!({ "agent_hex": agent_id });
                    let (status, body) = send_admin_uds_request(
                        &cli.admin_socket,
                        &secret,
                        "POST",
                        "/v1/admin/quarantine/lift",
                        Some(&payload),
                    )
                    .await?;
                    if status == 200 {
                        println!("[OK] Quarantine lifted for agent {}", agent_id);
                    } else {
                        eprintln!("[FAIL] HTTP {}: {}", status, body);
                    }
                }
                PolicyAction::Metrics => {
                    let (status, body) = send_admin_uds_request(
                        &cli.admin_socket,
                        &secret,
                        "GET",
                        "/v1/policy/metrics",
                        None,
                    )
                    .await?;
                    if status == 200 {
                        let val: Value = serde_json::from_str(&body)?;
                        println!("{}", serde_json::to_string_pretty(&val)?);
                    } else {
                        eprintln!("[FAIL] HTTP {}: {}", status, body);
                    }
                }
            }
        }

    // =====================================================================
        // 4. LIVE TERMINAL STREAM (`raqim tail`)
        // =====================================================================
        Commands::Tail { limit } => {
            let secret = load_admin_secret(&cli.admin_secret_path)?;
            println!("==================================================================");
            println!("Raqim Live Execution Stream (Polling {} — Ctrl+C to exit)", cli.admin_socket);
            println!("==================================================================");

            let mut seen_tx_ids: HashSet<String> = HashSet::new();
            let path = format!("/v1/admin/records/recent?limit={}", limit);

            loop {
                if let Ok((200, body)) =
                    send_admin_uds_request(&cli.admin_socket, &secret, "GET", &path, None).await
                {
                    if let Ok(records) = serde_json::from_str::<Vec<RecentRecord>>(&body) {
                        for rec in records {
                            if seen_tx_ids.insert(rec.tx_id.clone()) {
                                let badge = match rec.status.as_str() {
                                    "OUTCOME" => "\x1b[32m[OUTCOME]\x1b[0m",
                                    "INTENT" => "\x1b[33m[INTENT ]\x1b[0m",
                                    "DENIED" => "\x1b[31m[DENIED ]\x1b[0m",
                                    _ => "\x1b[36m[SYSTEM ]\x1b[0m",
                                };
                                println!(
                                    "{} tx={} agent={}.. path={} | {}",
                                    badge,
                                    &rec.tx_id[..12],
                                    &rec.agent_hex[..8.min(rec.agent_hex.len())],
                                    rec.intent_path,
                                    rec.text
                                );
                            }
                        }
                    }
                }
                tokio::time::sleep(Duration::from_millis(500)).await;
            }
        }
        
    
        // =====================================================================
        // 5. NDPA / GDPR IN-PLACE PII REDACTION (`raqim redact <tx_id>`)
        // =====================================================================
        Commands::Redact { tx_id } => {
            let secret = load_admin_secret(&cli.admin_secret_path)?;
            let path = format!("/v1/admin/redact/{}", tx_id.trim_start_matches("0x"));
            let (status, body) =
                send_admin_uds_request(&cli.admin_socket, &secret, "POST", &path, None).await?;
            if status == 200 {
                println!("[OK] {}", body);
            } else {
                eprintln!("[FAIL] HTTP {}: {}", status, body);
            }
        }

        // =====================================================================
        // 6. MANUAL COMPACTION TRIGGER (`raqim compact`)
        // =====================================================================
        Commands::Compact => {
            let secret = load_admin_secret(&cli.admin_secret_path)?;
            let (status, body) = send_admin_uds_request(
                &cli.admin_socket,
                &secret,
                "POST",
                "/v1/admin/compactor/trigger",
                None,
            )
            .await?;
            println!("HTTP {}: {}", status, body);
        }

        // =====================================================================
        // 7. OFFLINE EVIDENCE BUNDLE EXPORT (`raqim export --agent <hex>`)
        // =====================================================================
        Commands::Export { agent, out } => {
            let secret = load_admin_secret(&cli.admin_secret_path)?;
            let out_dir_str = out
                .clone()
                .unwrap_or_else(|| format!("./evidence-{}", &agent[..8.min(agent.len())]));
            let out_dir = Path::new(&out_dir_str);
            let proofs_dir = out_dir.join("proofs");
            fs::create_dir_all(&proofs_dir)?;

            let timeline_path = format!("/v1/admin/timeline/{}", agent);
            let (status, body) =
                send_admin_uds_request(&cli.admin_socket, &secret, "GET", &timeline_path, None)
                    .await?;
            if status != 200 {
                eprintln!("[FAIL] Could not export timeline (HTTP {}): {}", status, body);
                return Ok(());
            }

            let nodes: Vec<Value> = serde_json::from_str(&body)?;
            let http = reqwest::Client::new();
            let mut events_jsonl = String::new();
            let mut proof_count = 0usize;

            for node in &nodes {
                events_jsonl.push_str(&serde_json::to_string(node)?);
                events_jsonl.push('\n');

                if let Some(tx_num) = node["tx_id"].as_u64().or_else(|| None) {
                    let _ = tx_num;
                }
                // Extract u128 tx_id from JSON number or string
                let tx_raw = node["tx_id"].to_string().replace('"', "");
                if let Ok(tx_u128) = tx_raw.parse::<u128>() {
                    let tx_hex = format!("{:032x}", tx_u128);
                    let proof_url = format!("{}/v1/state/proof/{}", cli.data_url, tx_hex);
                    if let Ok(resp) = http.get(&proof_url).send().await {
                        if let Ok(proof_json) = resp.json::<Value>().await {
                            if proof_json["success"].as_bool() == Some(true) {
                                let proof_file = proofs_dir.join(format!("{}.json", tx_hex));
                                fs::write(
                                    proof_file,
                                    serde_json::to_string_pretty(&proof_json["proof"])?,
                                )?;
                                proof_count += 1;
                            }
                        }
                    }
                }
            }

            fs::write(out_dir.join("events.jsonl"), events_jsonl)?;
            println!(
                "[OK] Exported {} timeline events and {} cryptographic inclusion proofs to '{}'",
                nodes.len(),
                proof_count,
                out_dir.display()
            );
        }

    }

    Ok(())
}
