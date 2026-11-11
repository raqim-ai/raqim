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
    let http_client = Client::builder().build()?;

    match &cli.command {
        // 1. Crytographic Key Generation
        Commands::Keys {
            action:
                KeyAction::Forge {
                    name,
                    group,
                    count,
                    out_dir,
                },
        } => {
            println!("==========================================================");
            println!("Bismillah. Initiating Sovereign Fleet Forge... ");
            println!("Target Security Group [{}] ", group);
            println!("Requested Fleet Size [{}]", count);
            println!("Output Directory [{}]", out_dir);
            println!("===========================================================");

            let workspace = Path::new(out_dir);
            fs::create_dir_all(workspace)?;

            let mint_url = format!("{}/v1/admin/ca/mint", cli.daemon_url);
            let mut success_count = 0;

            for i in 1..=*count {
                let agent_alias: String = if *count > 1 {
                    format!("{}_{:02}", name, i)
                } else {
                    name.clone()
                };

                // Local cryptographic generation
                let mut csprng = OsRng;
                let signing_key = SigningKey::generate(&mut csprng);
                let public_key_bytes = signing_key.verifying_key().to_bytes();

                // Identity Hash Derivation
                let mut hasher = Hasher::new_derive_key("raqim.agent.v1.identity");
                hasher.update(&public_key_bytes);
                let mut derived_16_bytes = [0u8; 16];
                hasher.finalize_xof().fill(&mut derived_16_bytes);
                let agent_hex = hex::encode(derived_16_bytes);

                // Request Capability passport from the Daemon Control Plane
                let payload = json!({"agent_hex": agent_hex.clone(), "group": group.clone() });

                let req = http_client.post(&mint_url).json(&payload);

                match req.send().await {
                    Ok(response) if response.status().is_success() => {
                        let cert_hex: String = response.json().await?;
                        let cert_bytes = hex::decode(cert_hex)?;

                        // Atomic bundling in the Workspace
                        let key_path = workspace.join(format!("{}.pem", agent_alias));
                        let cert_path = workspace.join(format!("{}.cert", agent_alias));

                        fs::write(&key_path, signing_key.to_bytes())?;
                        fs::write(&cert_path, cert_bytes)?;

                        // Set strict Unix permissions for the private key
                        #[cfg(unix)]
                        {
                            use std::os::unix::fs::PermissionsExt;
                            fs::set_permissions(&key_path, fs::Permissions::from_mode(0o600))?;
                        }

                        println!("  [OK] Forged Agent: {} -> {} ", agent_alias, agent_hex);
                        success_count += 1;
                    }

                    Ok(response) => {
                        eprintln!(
                            "   [FAIL] Agent: [{}]: CA Minting Rejected - (Status Code: {})",
                            agent_alias,
                            response.status()
                        )
                    }

                    Err(e) => {
                        eprintln!(
                            "   [FAIL] Agent: {} - Daemom unreacheable at {} ({}) ",
                            agent_alias, mint_url, e
                        );
                    }
                }
            }

            println!(
                "\n✅ Fleet Forge Complete. Successfully generated {}/{} secure artifacts in {} ",
                success_count, count, out_dir
            );
        }

        // 2. Policy GateKeeper Management
        Commands::Policy {
            action: PolicyAction::List,
        } => {
            let url = format!("{}/v1/admin/quarantine", cli.daemon_url);
            let res = http_client.get(&url).send().await?;

            if res.status().is_success() {
                let records: Vec<QuarantineRecord> = res.json().await?;
                println!(
                    "🔒 Active Policy Quarantine Perimeters ({} Isolated):",
                    records.len()
                );
                if records.is_empty() {
                    println!("  None. All cryptographic gates clear.");
                } else {
                    for r in records {
                        println!(
                            "   -> Agent:{} | Target: {} | Reason: {}",
                            r.agent_hex, r.attemped_path, r.violation_type,
                        );
                    }
                }
            } else {
                eprintln!(
                    "❌ Operational Error: Failed to extract firewall status: {}",
                    res.status()
                );
            }
        }

        Commands::Policy {
            action: PolicyAction::Lift { agent_id, reason },
        } => {
            let url = format!("{}/v1/admin/quarantine/lift", cli.daemon_url);
            let res = http_client
                .post(&url)
                .json(&json!({"agent_hex": agent_id, "system_prompt_override": reason}))
                .send()
                .await?;

            if res.status().is_success() {
                println!(
                    "🔓 Quarantine lifted and context re-seeded for agent: {}",
                    agent_id
                );
            } else {
                eprintln!(
                    "❌ Operational Error: Reset signal refused by kernel: {}",
                    res.status()
                );
            }
        }

        // 3. Historical timeline Inspection
        Commands::TimeTravel { agent_id, tx_id: _ } => {
            let url = format!(
                "{}/v1/admin/time_travel/timeline/{}",
                cli.daemon_url, agent_id
            );

            let res = http_client.get(&url).send().await?;

            if res.status().is_success() {
                let timeline: Vec<serde_json::Value> = res.json().await?;
                println!(" ⌛ Historical Causal Timeline for Agent [{}]", agent_id);
                if timeline.is_empty() {
                    println!("  No commited states found for this identity");
                } else {
                    for (idx, node) in timeline.iter().enumerate() {
                        println!(
                            "  Step #{:02} | Tx: 0x{:032x} | Status: {:<12} | Payload: {}",
                            idx + 1,
                            node["tx_id"].as_u64().unwrap_or(0),
                            node["agent_status"].as_str().unwrap_or(""),
                            node[" payload_preview"].as_str().unwrap_or("")
                        );
                    }
                }
            } else {
                eprintln!("❌ Timeline query failed: HTTP {}", res.status());
            }
        }

        Commands::Cluster {
            action: ClusterAction::Info,
        } => {
            let url = format!("{}/v1/admin/cluster/info", cli.daemon_url);
            let res = http_client.get(&url).send().await?;

            if res.status().is_success() {
                let info: serde_json::Value = res.json().await?;
                println!("🌐 Raqim Core Kernel Metrics:");
                println!("  Node Identity Hash: {}", info["node_id"]);
                println!("  Highest Transaction: {}", info["highest_tx_id"]);
                println!(
                    "  Active WAL Size: {:.2}MB ({} bytes)",
                    info["wal_size_mb"].as_f64().unwrap_or(0.0),
                    info["wal_bytes"]
                );
                println!(
                    "  Cumulative CRRDT Ops  : {}  ",
                    info["cumulative_crdt_ops"]
                );
            } else {
                eprintln!("❌  Telemetry query failed: HTTP {}", res.status())
            }
        }

        Commands::Cluster {
            action: ClusterAction::Topology,
        } => {
            let url = format!("{}/v1/admin/cluster/topology", cli.daemon_url);

            let res = http_client.get(&url).send().await?;

            if res.status().is_success() {
                let shards: Vec<serde_json::Value> = res.json().await?;
                println!("🧠 Allocated Swarm Brain Shards  (Loro CRDT): ");
                for s in shards {
                    println!(
                        "  Shard Space: [{:<20}] | Timelines: {} | Ops: {:<8} | Est. RAM: {:.2} MB",
                        s["namespace"].as_str().unwrap_or(""),
                        s["active_timelines"],
                        s["total_crdt_operations"],
                        s["estimated_ram_mb"].as_f64().unwrap_or(0.0)
                    )
                }
            } else {
                eprintln!("❌  Topology query failed: HTTP {}", res.status());
            }
        }
    }

    Ok(())
}
