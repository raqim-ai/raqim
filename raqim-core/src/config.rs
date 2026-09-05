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
    topic: String,
    wal_path: String,
    manifest_path: String,
    witness_path: String,
    aegis_path: String,
    port: Option<u16>,
    dims: Option<i32>,
    limit: Option<usize>,
    embedder_type: Option<String>,
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
    license_key: String,
    openai_api_key: Option<String>,
}

#[derive(Parser, Debug)]
#[command(author, version, about = "Raqim Daemon configuration")]
pub struct CliArgs {
    #[arg(short, long)]
    pub topic: Option<String>,

    #[arg(short, long)]
    pub wal_path: Option<String>,

    #[arg(short, long)]
    pub manifest_path: Option<String>,

    #[arg(long)]
    pub witnes_path: Option<String>,

    #[arg(short, long)]
    pub lance_path: Option<String>,

    #[arg(short, long)]
    pub aegis_path: Option<String>,

    #[arg(short, long)]
    pub node_public_key_hex: Option<String>,

    #[arg(short, long)]
    pub embedder_type: Option<String>,

    #[arg(short, long)]
    pub openai_api_key: Option<String>,

    #[arg(long)]
    pub tenant_id: Option<String>,

    #[arg(long)]
    pub license_key: Option<String>,

    #[arg(short, long)]
    pub dims: Option<i32>,

    #[arg(long)]
    pub limit: Option<usize>,

    #[arg(short, long)]
    port: Option<u16>,
}

#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct RaqimConfig {
    pub topic: String,
    pub wal_path: String,
    pub manifest_path: String,
    pub lance_path: String,
    pub witness_path: String,
    pub aegis_path: String,
    pub table_name: String,
    pub tenant_id: String,
    pub license_key: String,
    pub node_public_key_hex: String,
    pub embedder_type: String,
    pub openai_api_key: String,
    pub dims: i32,
    pub limit: usize,
    pub port: u16,
}

impl Default for RaqimConfig {
    fn default() -> Self {
        Self {
            topic: "raqim_default".to_string(),
            wal_path: "./production.wal".to_string(),
            manifest_path: "./compactor.manifest.json".to_string(),
            witness_path: "./vault/witnesses".to_string(),
            lance_path: "./production_semantic.lancedb".to_string(),
            aegis_path: "./aegis.toml".to_string(),
            table_name: "agent_history".to_string(),
            tenant_id: "open_core_local".to_string(),
            license_key: "dev_move".to_string(),
            node_public_key_hex: "0000000000000000000000000000000000000000000000000000000000000000"
                .to_string(),

            embedder_type: "BGE-Base-EN-v1.5".to_string(),
            openai_api_key: "".to_string(),
            dims: 768,
            limit: 5,
            port: 8080,
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
                topic: proxy.daemon.topic,
                wal_path: proxy.daemon.wal_path,
                manifest_path: proxy.daemon.manifest_path,
                witness_path: proxy.daemon.witness_path,
                lance_path: proxy
                    .storage
                    .lance_path
                    .unwrap_or_else(|| "./production_semantic.lancedb".to_string()),
                aegis_path: proxy.daemon.aegis_path,
                table_name: proxy
                    .storage
                    .table_name
                    .unwrap_or_else(|| "agent_history".to_string()),
                tenant_id: proxy.identity.tenant_id,
                license_key: proxy.identity.license_key,
                node_public_key_hex: proxy.identity.node_pub_key_hex,
                embedder_type: proxy
                    .daemon
                    .embedder_type
                    .unwrap_or_else(|| "BGE-Base-EN-v1.5".to_string()),
                openai_api_key: proxy
                    .identity
                    .openai_api_key
                    .unwrap_or_else(|| "".to_string()),
                dims: proxy.daemon.dims.unwrap_or(384),
                limit: proxy.daemon.limit.unwrap_or(5),
                port: proxy.daemon.port.unwrap_or(8080),
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
        if let Some(t) = args.topic {
            config.topic = t;
        }
        if let Some(w) = args.wal_path {
            config.wal_path = w;
        }

        if let Some(m) = args.manifest_path {
            config.manifest_path = m;
        }

        if let Some(w_path) = args.witnes_path {
            config.witness_path = w_path;
        }

        if let Some(p_key) = args.node_public_key_hex {
            config.node_public_key_hex = p_key;
        }

        if let Some(e_type) = args.embedder_type {
            config.embedder_type = e_type
        }

        if let Some(o_api) = args.openai_api_key {
            config.openai_api_key = o_api
        }

        if let Some(l) = args.lance_path {
            config.lance_path = l;
        }
        if let Some(d) = args.dims {
            config.dims = d;
        }
        if let Some(t_id) = args.tenant_id {
            config.tenant_id = t_id;
        }

        if let Some(a) = args.aegis_path {
            config.aegis_path = a;
        }

        if let Some(key) = args.license_key {
            config.license_key = key;
        }

        if let Some(l) = args.limit {
            config.limit = l;
        }
        if let Some(p) = args.port {
            config.port = p
        }

        config
    }
}
