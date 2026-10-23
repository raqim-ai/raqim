use clap::Parser;
use serde::{Deserialize, Serialize};
use std::fs;
use std::path::Path;

#[derive(Deserialize)]
struct RaqimTomlProxy {
    daemon: DaemonSection,
    storage: StorageSection,
    identity: IdentitySection,
}

#[derive(Deserialize)]
struct DaemonSection {
    wal_path: String,
    manifest_path: String,
    witness_path: String,
    policy_path: String,
    port: Option<u16>,
    host: Option<String>,
}

#[derive(Deserialize)]
struct StorageSection {
    lance_path: Option<String>,
    table_name: Option<String>,
}

#[derive(Deserialize)]
struct IdentitySection {
    tenant_id: String,
    node_pub_key_hex: String,
}

#[derive(Parser, Debug)]
#[command(author, version, about = "Raqim Daemon configuration")]
pub struct CliArgs {
    #[arg(short, long)]
    pub wal_path: Option<String>,

    #[arg(short, long)]
    pub manifest_path: Option<String>,

    #[arg(long, alias = "witnes-path")]
    pub witness_path: Option<String>,

    #[arg(short, long)]
    pub policy_path: Option<String>,

    #[arg(short, long)]
    pub node_public_key_hex: Option<String>,

    #[arg(long)]
    pub tenant_id: Option<String>,

    #[arg(long)]
    pub limit: Option<usize>,

    #[arg(short, long)]
    port: Option<u16>,

    #[arg(long)]
    pub host: Option<String>,

    #[arg(long)]
    pub control_journal_path: Option<String>,

    #[arg(long)]
    pub checkpoint_path: Option<String>,
}

#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct RaqimConfig {
    pub wal_path: String,
    pub manifest_path: String,
    pub witness_path: String,
    pub policy_path: String,
    pub tenant_id: String,
    pub node_public_key_hex: String,
    pub port: u16,
    pub host: String,
    pub checkpoint_path: String,
    pub control_journal_path: String,
}

impl Default for RaqimConfig {
    fn default() -> Self {
        Self {
            wal_path: "./production.wal".to_string(),
            manifest_path: "./compactor.manifest.json".to_string(),
            witness_path: "./vault/witnesses".to_string(),
            policy_path: "./aegis.toml".to_string(),
            tenant_id: "open_core_local".to_string(),
            node_public_key_hex: "0000000000000000000000000000000000000000000000000000000000000000"
                .to_string(),

            port: 8080,
            host: "127.0.0.1".to_string(),
            checkpoint_path: "./vault/state_checkpoint.bin".to_string(),
            control_journal_path: "./vault/control_journal.bin".to_string(),
        }
    }
}

impl RaqimConfig {
    pub fn load_or_bootstrap() -> Self {
        let args = CliArgs::parse();
        let config_path = "raqim.toml";

        // Load from the disk or rely on default
        let mut config = if Path::new(config_path).exists() {
            let content =
                fs::read_to_string(config_path).expect("[FATAL] Failed to read raqim.toml");

            // Parse into sectional proxy structure
            let proxy: RaqimTomlProxy = toml::from_str(&content)
                .expect("[FATAL] Invalid TOML structural layout in raqim.toml ");

            RaqimConfig {
                wal_path: proxy.daemon.wal_path,
                manifest_path: proxy.daemon.manifest_path,
                witness_path: proxy.daemon.witness_path,

                policy_path: proxy.daemon.policy_path,

                tenant_id: proxy.identity.tenant_id,

                node_public_key_hex: proxy.identity.node_pub_key_hex,

                port: proxy.daemon.port.unwrap_or(8080),
                host: proxy.daemon.host.unwrap_or_else(|| "127.0.0.1".to_string()),
                checkpoint_path: "./vault/state_checkpoint.bin".to_string(),
                control_journal_path: "./vault/control_journal.bin".to_string(),
            }
        } else {
            let default_cfg = Self::default();
            let toml_string = toml::to_string(&default_cfg).unwrap();
            fs::write(config_path, toml_string).expect("Failed to bootstap raqim.toml");
            println!(
                "[SYSTEM] Bootstapped  default configuration at {}",
                config_path
            );
            default_cfg
        };

        //  THE OVERRIDE MATRIX: CLI Args always win if provided

        if let Some(w) = args.wal_path {
            config.wal_path = w;
        }

        if let Some(m) = args.manifest_path {
            config.manifest_path = m;
        }

        if let Some(w_path) = args.witness_path {
            config.witness_path = w_path;
        }

        if let Some(p_key) = args.node_public_key_hex {
            config.node_public_key_hex = p_key;
        }

        if let Some(t_id) = args.tenant_id {
            config.tenant_id = t_id;
        }

        if let Some(a) = args.policy_path {
            config.policy_path = a;
        }

        if let Some(p) = args.port {
            config.port = p
        }
        if let Some(h) = args.host {
            config.host = h;
        }
        if let Some(cj) = args.control_journal_path {
            config.control_journal_path = cj;
        }
        if let Some(cp) = args.checkpoint_path {
            config.checkpoint_path = cp;
        }

        config
    }
}
