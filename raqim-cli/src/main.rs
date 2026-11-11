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

    }

    
}
