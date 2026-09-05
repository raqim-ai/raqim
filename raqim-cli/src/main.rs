use blake3::Hasher;
use clap::{Parser, Subcommand};
use ed25519_dalek::SigningKey;
use rand::rngs::OsRng;
use reqwest::Client;
use serde::{Deserialize, Serialize};
use serde_json::json;
use std::path::Path;
use std::{fs, println};

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
    about = "Raqim OS Administrative Command Line Interface",
    version = "1.0.0"
)]
struct Cli {
    /// URL of the Raqim OS Daemon Control plane
    #[arg(short, long, default_value = "http://127.0.0.1:8081", global = true)]
    daemon_url: String,

    #[command(subcommand)]
    command: Commands,
}

#[derive(Subcommand)]
enum Commands {
    /// Cryptographic Identity & Fleet Provisioning
    Keys {
        #[command(subcommand)]
        action: KeyAction,
    },

    /// Aegis Cryptographic Firewall & Quarantine Diagnostics
    Aegis {
        #[command(subcommand)]
        action: AegisAction,
    },

    /// Temporal Routing & Reality Forking mechanics
    TimeTravel {
        #[arg(short, long)]
        agent_id: String,

        #[arg(short, long)]
        tx_id: Option<String>,
    },

    /// Swarm Infrastructure Observability and Telemetry Mapping
    Cluster {
        #[command(subcommand)]
        action: ClusterAction,
    },
}

#[derive(Subcommand)]
enum KeyAction {
    /// Batch forge cryptographic agents with signed CA
    Forge {
        // Base name for the agents (e.g finance_bot)
        #[arg(short, long)]
        name: String,

        /// The security group mapping declared in aegis.toml
        #[arg(short, long)]
        group: String,

        #[arg(short, long, default_value_t = 1)]
        count: u32,

        /// Target directory for the atomic artifact
        #[arg(short, long, default_value = "./ca-keys")]
        out_dir: String,
    },
}

#[derive(Subcommand)]
enum AegisAction {
    List,

    Lift {
        agent_id: String,
        #[arg(
            short,
            long,
            default_value = "Quarantine lifted via administrative CLI"
        )]
        reason: String,
    },
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

        // 2. Aegis GateKeeper Management
        Commands::Aegis {
            action: AegisAction::List,
        } => {
            let url = format!("{}/v1/admin/quarantine", cli.daemon_url);
            let res = http_client.get(&url).send().await?;

            if res.status().is_success() {
                let records: Vec<QuarantineRecord> = res.json().await?;
                println!(
                    "🔒 Active Aegis Quarantine Perimeters ({} Isolated):",
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

        Commands::Aegis {
            action: AegisAction::Lift { agent_id, reason },
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
