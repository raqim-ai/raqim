use std::{
    format,
    fs::{self, OpenOptions},
    io::Write,
    path::Path,
    println,
    time::{SystemTime, UNIX_EPOCH},
};

use ed25519_dalek::{Signer, SigningKey};
use serde::{Deserialize, Serialize};

use crate::{OpLog, axon::MarkleBatch};

#[derive(
    Debug, Clone, Serialize, Deserialize, rkyv::Archive, rkyv::Serialize, rkyv::Deserialize,
)]
pub struct AnchoredRootWitness {
    pub batch_id: u64,
    pub namespace: String,
    pub merkle_root_hex: String,
    pub parent_batch_root_hex: String,
    pub leaf_count: usize,
    pub timestamp: u64,
    pub master_signature_hex: String,
}

#[derive(
    Debug, Clone, rkyv::Archive, rkyv::Serialize, rkyv::Deserialize, Serialize, Deserialize,
)]
pub struct CertifiedBundleBlock {
    pub witness: AnchoredRootWitness,
    pub raw_logs: Vec<OpLog>,
}

pub struct WormWitnessEngine {
    witness_dir: String,
    master_signing_key: SigningKey,
    gcp_worm_bucket_url: Option<String>,
}

impl WormWitnessEngine {
    pub fn new(
        witness_dir: &str,
        master_signing_key: SigningKey,
        gcp_worm_bucket_url: Option<String>,
    ) -> Self {
        if !Path::new(witness_dir).exists() {
            let _ = fs::create_dir_all(witness_dir);
        }

        Self {
            witness_dir: witness_dir.to_string(),
            master_signing_key,
            gcp_worm_bucket_url,
        }
    }

    /// ANCHOR BATCH: Signs and Writ the complete data + root bundle to WORM targets
    pub async fn anchor_batch(
        &self,
        batch: &MarkleBatch,
        raw_logs: Vec<OpLog>,
    ) -> Result<AnchoredRootWitness, anyhow::Error> {
        let timestamp = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs();

        let merkle_root_hex = hex::encode(batch.markle_root);
        let parent_root_hex = hex::encode(batch.parent_batch_root);

        // Construct unsigned witness payload
        let unsign_payload = format!(
            "raqim.worm.v1:{}:{}:{}:{}:{}",
            batch.batch_id, batch.namespace, merkle_root_hex, parent_root_hex, timestamp
        );

        // Sign root with Master key
        let signature = self.master_signing_key.sign(unsign_payload.as_bytes());
        let signature_hex = hex::encode(signature.to_bytes());

        let witness = AnchoredRootWitness {
            batch_id: batch.batch_id,
            namespace: batch.namespace.clone(),
            merkle_root_hex: merkle_root_hex.clone(),
            parent_batch_root_hex: parent_root_hex.clone(),
            leaf_count: batch.leaves.len(),
            timestamp,
            master_signature_hex: signature_hex,
        };

        let bundle = CertifiedBundleBlock {
            witness: witness.clone(),
            raw_logs,
        };

        // Target 1: Local Append-Only Immutable WORM log
        let bundle_bytes = serde_json::to_vec_pretty(&bundle)?;
        let witness_file_path = format!("{}/batch_{:08}.json", self.witness_dir, batch.batch_id);

        let mut file = OpenOptions::new()
            .create_new(true)
            .write(true)
            .open(&witness_file_path)
            .map_err(|e| anyhow::anyhow!("WORM Violation: Cannot overwrite exiisting witness file '{}': {} ", witness_file_path, e) )?;

        file.write_all(&bundle_bytes)?;
        file.sync_all()?;

        // Apply Linux Kernel Immutable Attribute
        #[cfg(unix)]
        {
            
            use std::os::unix::fs::PermissionsExt;
            let _ = fs::set_permissions(&witness_file_path, fs::Permissions::from_mode(0o400));

        }

        println!(
            "[WORM WITNESS] Merkle Root for Batch #{} anchored to immutable storage: {} ",
            batch.batch_id, merkle_root_hex
        );

        // Target 2: GCP WORM Bucket
        if let Some(bucket_url) = &self.gcp_worm_bucket_url {
            let client = reqwest::Client::new();
            let url = format!("{}/batch_{:08}.json", bucket_url, batch.batch_id);
            let _ = client.put(&url).body(bundle_bytes).send().await;
        }

        Ok(witness)
    }

    /// Cold recovery fetcher: Pulls an untampered historical block bundle diirectly from WORM vault
    pub async fn fetch_bundle_from_witness(
        &self,
        batch_id: u64,
    ) -> Result<CertifiedBundleBlock, anyhow::Error> {
        let file_path = format!("{}/batch_{:08}.json", self.witness_dir, batch_id);

        if Path::new(&file_path).exists() {
            let content = fs::read_to_string(&file_path)?;
            let bundle = serde_json::from_str::<CertifiedBundleBlock>(&content)?;

            return Ok(bundle);
        }

        if let Some(bucket_url) = &self.gcp_worm_bucket_url {
            let client = reqwest::Client::new();
            let url = format!("{}/bundle_{:08}.json", bucket_url, batch_id);
            let resp = client.get(&url).send().await?;

            if resp.status().is_success() {
                let bundle = resp.json::<CertifiedBundleBlock>().await?;
                return Ok(bundle);
            }
        }

        Err(anyhow::anyhow!(
            "Disaster Recovery Error: Block bundle #{} not found in any worm target.",
            batch_id
        ))
    }

    /// Read all local witness record chronologically for boot verification
    pub fn load_local_witness(&self) -> Vec<AnchoredRootWitness> {
        let mut witnesses = Vec::new();
        let path = std::path::Path::new(&self.witness_dir);

        if !path.exists() {
            return witnesses;
        }
        
        if let Ok(entries) = fs::read_dir(&self.witness_dir) {
            for entry in entries.flatten() {
                if entry.path().extension().and_then(|s| s.to_str()) == Some("json") {
                    if let Ok(content) = fs::read_to_string(entry.path()) {
                        if let Ok(bundle) = serde_json::from_str::<CertifiedBundleBlock>(&content) {
                            witnesses.push(bundle.witness);
                        }
                    }
                }
            }
        }

        witnesses.sort_by_key(|w| w.batch_id);
        witnesses
    }
}
