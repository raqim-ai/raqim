use crate::{
    witness::{AnchoredRootWitness, WormWitnessEngine},
    ExecutionRecord, RecentRecord,
};
use blake3::Hasher;
use dashmap::DashMap;
use parking_lot::RwLock;
use serde::{Deserialize, Serialize};
use std::{
    eprintln, format, println,
    sync::{atomic::Ordering::SeqCst, Arc},
};

/// A completed cryptographic audit checkpoint batch ready for ledger immutability
#[derive(
    Debug, Clone, Serialize, Deserialize, rkyv::Archive, rkyv::Serialize, rkyv::Deserialize,
)]
pub struct MerkleBatch {
    pub batch_id: u64,
    pub namespace: String,
    pub merkle_batch: [u8; 32],
    pub parent_batch_root: [u8; 32],
    pub leaves: Vec<[u8; 32]>,
}

/// A verifiable inclusion path mapping a specific transaction leaf to the root
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct InclusionProof {
    pub tx_id_hex: String,
    pub leaf_index: usize,
    pub sibling_hashes_hex: Vec<String>,

    pub merkle_root_hex: String,
    pub parent_batch_root_hex: String,
    pub batch_id: u64,
    pub is_active_buffer: bool,
    pub payload_hash_hex: String,
}

/// Thread-safe active workspace buffer for a single namespace
pub struct ActiveTreeBuffer {
    pub current_batch_id: u64,
    pub parent_batch_root: [u8; 32],
    pub accumulated_leaves: Vec<[u8; 32]>,
    pub accumulated_records: Vec<ExecutionRecord>,
    pub accumulated_tx_ids: Vec<u128>,
}

/// The active Governance GateKeeper.
pub struct MerkleEngine {
    /// Thread-safe active memory arenas partiioned per swarm namespce
    pub active_buffers: DashMap<String, Arc<RwLock<ActiveTreeBuffer>>>,

    /// The global completed block ledger for historical lookups
    pub batch_archive: DashMap<u64, MerkleBatch>,
    pub global_batch_counter: std::sync::atomic::AtomicU64,

    // Fast-path map: tx_id(u128) -> (batch_id, leaf_index)
    pub tx_to_location: DashMap<u128, (u64, usize)>,
}

impl MerkleEngine {
    pub fn new() -> Self {
        Self {
            active_buffers: DashMap::new(),
            batch_archive: DashMap::new(),
            tx_to_location: DashMap::new(),
            global_batch_counter: std::sync::atomic::AtomicU64::new(0),
        }
    }

    /// Computed discrete leaf hash from `payload_hash` and `agent_id`.
    pub fn compute_leaf_hash(payload_hash: &[u8; 32], agent_id: &[u8; 16]) -> [u8; 32] {
        let mut leaf_hasher = Hasher::new_derive_key("raqim.merkle.v1.leaf");
        leaf_hasher.update(payload_hash);
        leaf_hasher.update(agent_id);
        leaf_hasher.finalize().into()
    }

    /// Cryptographically seals an `ExecutionRecord` into the namespace's Merkle tree. Triggers automatic batch crystallization when leaf capacity reaches 1,024
    pub fn seal_record(
        &self,
        mut record: ExecutionRecord,
    ) -> (ExecutionRecord, Option<MerkleBatch>) {
        let namespace = record.namespace.clone();

        // Compute the discrete leaf cryptographic hash using domain separation
        let leaf_hash = Self::compute_leaf_hash(&record.payload_hash, &record.agent_id);

        // Fetch parent root hash for temporal linkage
        let parent_root = self
            .active_buffers
            .get(&namespace)
            .map(|b| b.read().parent_batch_root)
            .unwrap_or([0u8; 32]);

        record.previous_root = parent_root;
        record.leaf_hash = leaf_hash;

        // Delegate to internal ingestion helper
        let batch = self.ingest_leaf_internal(&namespace, leaf_hash, record.clone());

        (record, batch)
    }

    /// Internal Engine loop: Condenses an arbitrary array of leaf hashes into a single Merkle Root
    pub fn compute_merkle_root(leaves: &[[u8; 32]]) -> [u8; 32] {
        if leaves.is_empty() {
            return [0u8; 32];
        }

        let mut current_level = leaves.to_vec();
        while current_level.len() > 1 {
            let mut next_level: Vec<[u8; 32]> = Vec::with_capacity((current_level.len() + 1) / 2);

            for chunk in current_level.chunks(2) {
                if chunk.len() == 2 {
                    let mut hasher = Hasher::new_derive_key("raqim.merkle.v1.node");
                    hasher.update(&chunk[0]);
                    hasher.update(&chunk[1]);
                    next_level.push(hasher.finalize().into());
                } else {
                    // Odd element cleanujp
                    let mut hasher = Hasher::new_derive_key("raqim.merkle.v1.node");
                    hasher.update(&chunk[0]);
                    hasher.update(&chunk[0]);
                    next_level.push(hasher.finalize().into());
                }
            }

            current_level = next_level;
        }

        current_level[0]
    }

    /// Proof generator: 0(log N) proof extraction for any tx_id (Archiived or Active buffer)
    pub fn generate_proof_for_tx(&self, tx_id: u128) -> Option<InclusionProof> {
        let tx_id_hex = format!("{:032x}", tx_id);

        // Check Location  Index
        if let Some(location) = self.tx_to_location.get(&tx_id) {
            let (batch_id, leaf_index) = *location.value();

            // Path A: Query Completed batch archive
            if let Some(batch) = self.batch_archive.get(&batch_id) {
                let proof_nodes = Self::exract_sibling_path(&batch.leaves, leaf_index);
                return Some(InclusionProof {
                    tx_id_hex,
                    leaf_index,
                    sibling_hashes_hex: proof_nodes.iter().map(|h| hex::encode(h)).collect(),
                    merkle_root_hex: (hex::encode(batch.merkle_batch)),
                    parent_batch_root_hex: hex::encode(batch.parent_batch_root),
                    batch_id,
                    is_active_buffer: false,
                    payload_hash_hex: String::new(),
                });
            }
        }

        // Path B: Un-crystallized Active Workspace Buffer search
        for entry in self.active_buffers.iter() {
            let buffer = entry.value().read();
            if let Some(leaf_index) = buffer.accumulated_tx_ids.iter().position(|&id| id == tx_id) {
                let active_root = Self::compute_merkle_root(&buffer.accumulated_leaves);
                let proof_nodes = Self::exract_sibling_path(&buffer.accumulated_leaves, leaf_index);
                let payload_hash_hex = buffer
                    .accumulated_records
                    .get(leaf_index)
                    .map(|r| hex::encode(r.payload_hash))
                    .unwrap_or_default();

                return Some(InclusionProof {
                    tx_id_hex,
                    leaf_index,
                    sibling_hashes_hex: proof_nodes.iter().map(|h| hex::encode(h)).collect(),
                    merkle_root_hex: hex::encode(active_root),
                    parent_batch_root_hex: hex::encode(buffer.parent_batch_root),
                    batch_id: buffer.current_batch_id,
                    is_active_buffer: true,
                    payload_hash_hex,
                });
            }
        }

        None
    }

    fn exract_sibling_path(leaves: &[[u8; 32]], leaf_index: usize) -> Vec<[u8; 32]> {
        let mut sibling_hashes = Vec::new();
        let mut current_level = leaves.to_vec();
        let mut index = leaf_index;

        while current_level.len() > 1 {
            let mut next_level = Vec::with_capacity((current_level.len() + 1) / 2);

            for chunk in current_level.chunks(2) {
                let mut hasher = Hasher::new_derive_key("raqim.merkle.v1.node");
                if chunk.len() == 2 {
                    hasher.update(&chunk[0]);
                    hasher.update(&chunk[1]);
                } else {
                    hasher.update(&chunk[0]);
                    hasher.update(&chunk[0]);
                }

                next_level.push(hasher.finalize().into());
            }

            let sibling_idx = if index % 2 == 0 {
                if index + 1 < current_level.len() {
                    index + 1
                } else {
                    index
                }
            } else {
                index - 1
            };

            sibling_hashes.push(current_level[sibling_idx]);
            current_level = next_level;
            index /= 2;
        }

        sibling_hashes
    }

    /// Single Core Ingestion Engine: Appends leaf hash, checks batch capacity, and crystallizes Merkle Root if full
    fn ingest_leaf_internal(
        &self,
        namespace: &str,
        leaf_hash: [u8; 32],
        record: ExecutionRecord,
    ) -> Option<MerkleBatch> {
        let tx_id = record.tx_id;

        // Pass 1: Acquire reference to the namespace buffer
        let buffer_arc = self
            .active_buffers
            .entry(namespace.to_string())
            .or_insert_with(|| {
                Arc::new(RwLock::new(ActiveTreeBuffer {
                    current_batch_id: self.global_batch_counter.fetch_add(1, SeqCst),
                    parent_batch_root: [0u8; 32],
                    accumulated_leaves: Vec::with_capacity(1024),
                    accumulated_records: Vec::with_capacity(1024),
                    accumulated_tx_ids: Vec::with_capacity(1024),
                }))
            })
            .value()
            .clone();

        let mut buffer = buffer_arc.write();

        // Track 0(1) TxID location map
        self.tx_to_location.insert(
            tx_id,
            (buffer.current_batch_id, buffer.accumulated_leaves.len()),
        );

        buffer.accumulated_leaves.push(leaf_hash);
        buffer.accumulated_records.push(record);
        buffer.accumulated_tx_ids.push(tx_id);

        // Cap Checkpoint: If capacity hits 1,024 leaves, crystallize the Merkle Tree
        if buffer.accumulated_leaves.len() >= 1024 {
            let root = Self::compute_merkle_root(&buffer.accumulated_leaves);

            let completed_batch = MerkleBatch {
                batch_id: buffer.current_batch_id,
                namespace: namespace.to_string(),
                merkle_batch: root,
                parent_batch_root: buffer.parent_batch_root,
                leaves: buffer.accumulated_leaves.clone(),
            };

            //  Archive completed block for historical query proofs
            self.batch_archive
                .insert(completed_batch.batch_id, completed_batch.clone());

            // Advace the StatePipeline cleanly
            buffer.parent_batch_root = root;
            buffer.current_batch_id = self.global_batch_counter.fetch_add(1, SeqCst);
            buffer.accumulated_leaves.clear();
            buffer.accumulated_records.clear();
            buffer.accumulated_tx_ids.clear();

            return Some(completed_batch);
        }

        None
    }

    /// THE PHOENIX CRASH RECOVERY COMPONENT
    pub fn hydrate_from_recovery(&self, record: &ExecutionRecord) {
        let namespace = record.namespace.clone();
        let _ = self.ingest_leaf_internal(&namespace, record.leaf_hash, record.clone());
    }

    /// Exports the full cryptographic Merkle DAG state for checkpointing
    pub fn export_checkpoint_state(&self) -> crate::checkpoint::MerkleCheckpointState {
        let global_batch_counter = self.global_batch_counter.load(SeqCst);
        let mut active_buffers = Vec::new();
        for entry in self.active_buffers.iter() {
            let ns = entry.key().clone();
            let buf = entry.value().read();
            active_buffers.push(crate::checkpoint::ActiveTreeBufferSnapshot {
                namespace: ns,
                current_batch_id: buf.current_batch_id,
                parent_batch_root: buf.parent_batch_root,
                accumulated_leaves: buf.accumulated_leaves.clone(),
                accumulated_records: buf.accumulated_records.clone(),
                accumulated_tx_ids: buf.accumulated_tx_ids.clone(),
            });
        }
        let batch_archive: Vec<MerkleBatch> = self
            .batch_archive
            .iter()
            .map(|e| e.value().clone())
            .collect();
        crate::checkpoint::MerkleCheckpointState {
            global_batch_counter,
            active_buffers,
            batch_archive,
        }
    }

    /// Hydrates the cryptographic Merkle DAG from a checkpoint
    pub fn hydrate_from_checkpoint(&self, state: crate::checkpoint::MerkleCheckpointState) {
        self.global_batch_counter
            .store(state.global_batch_counter, SeqCst);
        for snap in state.active_buffers {
            let buffer = ActiveTreeBuffer {
                current_batch_id: snap.current_batch_id,
                parent_batch_root: snap.parent_batch_root,
                accumulated_leaves: snap.accumulated_leaves,
                accumulated_records: snap.accumulated_records,
                accumulated_tx_ids: snap.accumulated_tx_ids,
            };
            self.active_buffers
                .insert(snap.namespace, Arc::new(RwLock::new(buffer)));
        }
        for batch in state.batch_archive {
            self.batch_archive.insert(batch.batch_id, batch);
        }
    }

    pub fn verify_inclusion_by_payload_hash(
        payload_hash: &[u8; 32],
        agent_id: &[u8; 16],
        proof: &InclusionProof,
    ) -> bool {
        let mut current_hash = Self::compute_leaf_hash(payload_hash, agent_id);
        let mut index = proof.leaf_index;

        for sibling_hex in &proof.sibling_hashes_hex {
            let Ok(sibling_bytes) = hex::decode(sibling_hex) else {
                return false;
            };
            if sibling_bytes.len() != 32 {
                return false;
            }

            let mut hasher = Hasher::new_derive_key("raqim.merkle.v1.node");
            if index % 2 == 0 {
                hasher.update(&current_hash);
                hasher.update(&sibling_bytes);
            } else {
                hasher.update(&sibling_bytes);
                hasher.update(&current_hash);
            }

            current_hash = hasher.finalize().into();
            index /= 2;
        }

        hex::encode(current_hash) == proof.merkle_root_hex
    }

    /// Validates inclusion proof using raw payload + proof data
    pub fn verify_inclusion(
        payload_bytes: &[u8],
        agent_id: &[u8; 16],
        proof: &InclusionProof,
    ) -> bool {
        let payload_hash = ExecutionRecord::compute_salted_payload_hash(payload_bytes);

        Self::verify_inclusion_by_payload_hash(&payload_hash, agent_id, proof)
    }

    /// FORENSIC INTEGRITY COMPONENT: Execute local disk cross-checks against anchored WORM witness on startup
    pub async fn execute_forensic_boot_audit(
        &self,
        witnesses: &[AnchoredRootWitness],
        witness_engine: &WormWitnessEngine,
    ) -> Result<(), anyhow::Error> {
        println!(
            "[PHOENIX AUDIT] Commencing cryptographic verification of local ledger blocks against WORM anchors... "
        );

        for witness in witnesses {
            let mut local_valid = false;

            if let Some(local_batch) = self.batch_archive.get(&witness.batch_id) {
                let local_root_hex = hex::encode(local_batch.merkle_batch);
                if local_root_hex == witness.merkle_root_hex {
                    local_valid = true;
                }
            }

            // Critical disaster Recovery Override: Local delta is corrupted, altred or missing compeltely
            if !local_valid {
                eprintln!(
                    "\n[PHOENIX RED ALERT] Cryptographic mismatch or missing data detected on local block Batch #{}! Disk state is comprised. Triggering WORM Vault Rollback...",
                    witness.batch_id
                );

                // Fetch the absolute, untampered historical block bundle from our WORM Cloud/Local Vault
                let intact_bundle = witness_engine
                    .fetch_bundle_from_witness(witness.batch_id)
                    .await?;

                // Re-validate the fetched bundle's cryptigraphic proof to ensure the witness itself wasn't corrupted
                let computed_root = Self::compute_merkle_root(
                    &intact_bundle
                        .raw_logs
                        .iter()
                        .map(|l| l.leaf_hash)
                        .collect::<Vec<_>>(),
                );
                if hex::encode(computed_root) != witness.merkle_root_hex {
                    return Err(anyhow::anyhow!(
                        "FATAL: WORM witness vault payload failed self-authentication. Perimeter compromised. Terminating."
                    ));
                }
                // Repair the local memory map by rolling back to the intact WORM block data state
                let recomputed_batch = MerkleBatch {
                    batch_id: intact_bundle.witness.batch_id,
                    namespace: intact_bundle.witness.namespace.clone(),
                    merkle_batch: computed_root,
                    parent_batch_root: hex::decode(&intact_bundle.witness.parent_batch_root_hex)?
                        .try_into()
                        .unwrap_or([0u8; 32]),
                    leaves: intact_bundle.raw_logs.iter().map(|l| l.leaf_hash).collect(),
                };

                // Overwrite the corrupted local memory slot with pristine data
                self.batch_archive
                    .insert(recomputed_batch.batch_id, recomputed_batch);

                // Re-map transaction indices for the repaired block logs
                for (idx, log) in intact_bundle.raw_logs.iter().enumerate() {
                    self.tx_to_location
                        .insert(log.tx_id, (witness.batch_id, idx));
                }

                println!(
                    "[PHOENIX AUDIT] Re-mapped and restored 1,024 transaction logs from WORM vault for Batch #{}. ",
                    witness.batch_id
                );
            }
        }

        println!(
            "[PHOENIX AUDIT] Audit Complete. All local historical blocks verified and synchronized with WORM anchors."
        );
        Ok(())
    }

    /// Returns the exact count of all leaves stored across completed batches and active buffers
    pub fn get_total_leaves(&self) -> usize {
        let archived_leaves: usize = self
            .batch_archive
            .iter()
            .map(|e| e.value().leaves.len())
            .sum();

        let active_leaves: usize = self
            .active_buffers
            .iter()
            .map(|e| e.value().read().accumulated_leaves.len())
            .sum();

        archived_leaves + active_leaves
    }

    /// Returns the highest tx_id currently indexed in the merkle-dag
    pub fn get_latest_tx_id(&self) -> u128 {
        self.tx_to_location
            .iter()
            .map(|e| *e.key())
            .max()
            .unwrap_or(0)
    }

    /// Collects recent Record from active RAM buffers across all namespaces
    pub fn get_recent_record_ram(&self, limit: usize) -> Vec<RecentRecord> {
        let mut records = Vec::new();
        for entry in self.active_buffers.iter() {
            let buffer = entry.value().read();
            for rec in buffer.accumulated_records.iter() {
                records.push(RecentRecord {
                    tx_id: format!("{:032x}", rec.tx_id),
                    tx_id_hex: format!("0x{:032x}", rec.tx_id),
                    agent_hex: hex::encode(rec.agent_id),
                    intent_path: rec.namespace.clone(),
                    text: rec.payload_text_preview(),
                    status: rec.stage.as_str().to_string(),
                    timestamp: rec.timestamp,
                });
            }
        }
        records.sort_by(|a, b| b.timestamp.cmp(&a.timestamp));
        records.truncate(limit);
        records
    }
}
