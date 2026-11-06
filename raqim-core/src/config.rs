use clap::Parser;
use serde::{Deserialize, Serialize};
use std::fs;
use std::path::Path;

#[derive(Parser, Debug)]
#[command(
    author,
    version,
    about = "Raqim Execution-Integrity Daemon Configuration "
)]
pub struct CliArgs {
    #[arg(short, long)]
    pub wal_path: Option<String>,

    #[arg(short, long)]
    pub manifest_path: Option<String>,

    #[arg(long, alias = "witnes-path")]
    pub witness_path: Option<String>,

    #[arg(short, long, alias = "policy_path")]
    pub policy_path: Option<String>,

    #[arg(long)]
    pub tenant_id: Option<String>,

    #[arg(short, long)]
    port: Option<u16>,

    #[arg(long)]
    pub host: Option<String>,

    #[arg(long)]
    pub control_journal_path: Option<String>,

    #[arg(long)]
    pub checkpoint_path: Option<String>,

    #[arg(long)]
    pub cold_store_path: Option<String>,

    #[arg(long)]
    pub admin_socket_path: Option<String>,

    #[arg(long)]
    pub worm_remote_url: Option<String>,
}

/// Declarrative config for a named upstream reverse-proxy connectoe
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ConnectorConfig {
    /// Connector identifier in `/v1/egress/{name}/{*path}` (e.g., "openai", "paystack").
    pub name: String,
    /// Base upstream URL (e.g., "https://api.openai.com" or "https://api.paystack.co").
    pub upstream_url: String,
    /// Name of the host environment variable holding the real API secret (e.g., "OPENAI_API_KEY").
    pub secret_env: String,
    /// Upstream HTTP header name for credential injection (default: "Authorization").
    #[serde(default = "default_auth_header")]
    pub auth_header: String,
    /// Prefix prepended to the secret value (default: "Bearer "; use "" for raw key headers like "x-api-key").
    #[serde(default = "default_auth_prefix")]
    pub auth_prefix: String,
    /// Optional idempotency header name (e.g., Some("Idempotency-Key")).
    /// When set, Raqim automatically injects the write-ahead Intent's unique UUIDv7 `tx_id`!
    pub idempotency_header: Option<String>,
    /// Explicit method + path allowlist (e.g., ["POST /v1/chat/completions", "POST /transfer"]).
    /// Any route not matching this list is rejected and quarantined!
    #[serde(default)]
    pub allowed_routes: Vec<String>,
    /// RFC 6901 JSON Pointers (e.g., ["/card/cvv", "/bvn"]) to scrub before writing to WAL.
    #[serde(default)]
    pub redact_pointers: Vec<String>,
}

fn default_auth_header() -> String {
    "Authorization".to_string()
}

fn default_auth_prefix() -> String {
    "Bearer ".to_string()
}

/// Unified Configuration Schema ( Used identically for both TOML read & write )
#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct RaqimConfig {
    pub tenant_id: String,
    pub port: u16,
    pub host: String,
    pub wal_path: String,
    pub manifest_path: String,
    pub witness_path: String,
    pub policy_path: String,
    pub checkpoint_path: String,
    pub control_journal_path: String,
    pub cold_store_path: String,
    pub admin_socket_path: String,
    pub worm_remote_url: Option<String>,
    pub connectors: Vec<ConnectorConfig>,
}

impl Default for RaqimConfig {
    fn default() -> Self {
        Self {
            tenant_id: "local_sovereign".to_string(),
            port: 8080,
            host: "127.0.0.1".to_string(),
            wal_path: "./production.wal".to_string(),
            manifest_path: "./compaction.manifest.json".to_string(),
            witness_path: "./vault/witnesses".to_string(),
            policy_path: "./policy.toml".to_string(),
            checkpoint_path: "./vault/state_checkpoint.bin".to_string(),
            control_journal_path: "./vault/control_journal.bin".to_string(),
            cold_store_path: "./vault/cold_segments".to_string(),
            admin_socket_path: "./vault/admin.sock".to_string(),
            worm_remote_url: None,
            connectors: Vec::new(),
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

            toml::from_str::<RaqimConfig>(&content)
                .expect("[FATAL] Invalid TOML syntax in raqim.toml ")
        } else {
            let default_cfg = Self::default();
            let toml_string = toml::to_string_pretty(&default_cfg)
                .expect("Faild to serialize default RaqimConfig");
            fs::write(config_path, toml_string).expect("Failed to bootstap raqim.toml");
            println!(
                "[SYSTEM] Bootstapped  default configuration at {}",
                config_path
            );
            default_cfg
        };

        // The CLI override matrix

        if let Some(w) = args.wal_path {
            config.wal_path = w;
        }
        if let Some(m) = args.manifest_path {
            config.manifest_path = m;
        }
        if let Some(wp) = args.witness_path {
            config.witness_path = wp;
        }
        if let Some(ap) = args.policy_path {
            config.policy_path = ap;
        }
        if let Some(tid) = args.tenant_id {
            config.tenant_id = tid;
        }
        if let Some(p) = args.port {
            config.port = p;
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
        if let Some(cs) = args.cold_store_path {
            config.cold_store_path = cs;
        }
        if let Some(sock) = args.admin_socket_path {
            config.admin_socket_path = sock;
        }
        if let Some(url) = args.worm_remote_url {
            config.worm_remote_url = Some(url);
        }

        config
    }

    pub fn get_connector(&self, name: &str) -> Option<&ConnectorConfig> {
        self.connectors.iter().find(|c| c.name == name)
    }
}
