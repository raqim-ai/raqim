use crate::{
    witness::{AnchoredRootWitness, WormWitnessEngine},
    ExecutionRecord, ExecutionStage, RecentRecord,
};
use blake3::Hasher;
use dashmap::DashMap;
use ed25519_dalek::{Signature, Verifier, VerifyingKey};
use parking_lot::RwLock;
use rkyv::bytecheck::Verify;
use serde::{Deserialize, Serialize};
use std::{
    eprintln, format, println,
    sync::{
        atomic::Ordering::{self, SeqCst},
        Arc,
    },
};

/// A completed cryptographic audit checkpoint batch ready for ledger immutability
#[derive(
    Debug, Clone, Serialize, Deserialize, rkyv::Archive, rkyv::Serialize, rkyv::Deserialize,
)]
pub struct MerkleBatch {
    pub batch_id: u64,
    pub namespace: String,
    pub merkle_root: [u8; 32],
    pub parent_batch_root: [u8; 32],
    pub leaves: Vec<[u8; 32]>,
}

/// Complete, self-veriifying Inclusion Proof committing to the entire action envelope
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct InclusionProof {
    pub tx_id_hex: String,
    pub agent_hex: String,
    pub step_ordinal: u64,
    pub timestamp: i64,
    pub stage: u8,
    pub namespace: String,
    pub call_sig_hex: String,
    pub payload_salt_hex: String,
    pub payload_hash_hex: String,
    pub leaf_index: usize,
    pub total_leaves_in_tree: usize,
    pub sibling_hashes_hex: Vec<String>,
    pub merkle_root_hex: String,
    pub parent_batch_root_hex: String,
    pub batch_id: u64,
    pub is_active_buffer: bool,
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

    /// Full Action Envelope Commitment (`raqim.merkle.v2.leaf`)
    pub fn compute_envelope_leaf_hash(
        tx_id: u128,
        agent_id: &[u8; 16],
        step_ordinal: u64,
        timestamp: i64,
        stage: ExecutionStage,
        namespace: &str,
        call_signature_hash: &[u8; 32],
        payload_hash: &[u8; 32],
    ) -> [u8; 32] {
        let mut leaf_hasher = Hasher::new_derive_key("raqim.merkle.v2.leaf");
        leaf_hasher.update(&tx_id.to_le_bytes());
        leaf_hasher.update(agent_id);
        leaf_hasher.update(&step_ordinal.to_le_bytes());
        leaf_hasher.update(&timestamp.to_le_bytes());
        leaf_hasher.update(&[stage as u8]);
        let ns_bytes = namespace.as_bytes();
        leaf_hasher.update(&(ns_bytes.len() as u32).to_le_bytes());
        leaf_hasher.update(ns_bytes);
        leaf_hasher.update(call_signature_hash);
        leaf_hasher.update(payload_hash);
        leaf_hasher.finalize().into()
    }

    pub fn compute_record_leaf_hash(rec: &ExecutionRecord) -> [u8; 32] {
        Self::compute_envelope_leaf_hash(
            rec.tx_id,
            &rec.agent_id,
            rec.step_ordinal,
            rec.timestamp,
            rec.stage,
            &rec.namespace,
            &rec.call_signature_hash,
            &rec.payload_hash,
        )
    }

    /// Stage 1 of two-phase ingestion: computes `leaf_hash` and `previous_root` BEFORE WAL append before  mutating  in-mem treee state (so failed WAL writes never pollute RAM)
    pub fn prepare_record(&self, mut record: ExecutionRecord) -> ExecutionRecord {
        let leaf_hash = Self::compute_record_leaf_hash(&record);
        let parent_root = self
            .active_buffers
            .get(&record.namespace)
            .map(|b| b.read().parent_batch_root)
            .unwrap_or([0u8; 32]);

        record.previous_root = parent_root;
        record.leaf_hash = leaf_hash;

        record
    }

    /// Stage 2 of two-phase ingestion: commits a record into Merkle tree after WAl durability succeeds
    pub fn commit_prepared_record(
        &self,
        record: ExecutionRecord,
    ) -> Option<(MerkleBatch, Vec<ExecutionRecord>)> {
        let namespace = record.namespace.clone();
        let leaf_hash = record.leaf_hash;
        self.ingest_leaf_internal(&namespace, leaf_hash, record)
    }

    /// Convenience wrapper for tests / sync callers.
    pub fn seal_record(
        &self,
        record: ExecutionRecord,
    ) -> (ExecutionRecord, Option<(MerkleBatch, Vec<ExecutionRecord>)>) {
        let prepared = self.prepare_record(record);
        let batch_opt = self.commit_prepared_record(prepared.clone());

        (prepared, batch_opt)
    }

    /// Computees Merkle root using distict domain separation for odd boundary vs paired nodes.
    /// Fixes the odd-leaf duplication collision
    pub fn compute_merkle_root(leaves: &[[u8; 32]]) -> [u8; 32] {
        if leaves.is_empty() {
            return [0u8; 32];
        }

        let mut current_level = leaves.to_vec();
        while current_level.len() > 1 {
            let mut next_level: Vec<[u8; 32]> = Vec::with_capacity((current_level.len() + 1) / 2);

            for chunk in current_level.chunks(2) {
                if chunk.len() == 2 {
                    let mut hasher = Hasher::new_derive_key("raqim.merkle.v2.node");
                    hasher.update(&chunk[0]);
                    hasher.update(&chunk[1]);
                    next_level.push(hasher.finalize().into());
                } else {
                    // Unpaired odd node uses a distict domain key over single child
                    let mut hasher = Hasher::new_derive_key("raqim.merkle.v1.node_odd");
                    hasher.update(&chunk[0]);
                    next_level.push(hasher.finalize().into());
                }
            }

            current_level = next_level;
        }

        current_level[0]
    }

    /// Flushes any buffer that has pending leaves
    pub fn flush_all_active_buffers(&self) -> Vec<(MerkleBatch, Vec<ExecutionRecord>)> {
        let mut flushed = Vec::new();
        for entry in self.active_buffers.iter() {
            let ns = entry.key().clone();
            let mut buffer = entry.value().write();

            if !buffer.accumulated_leaves.is_empty() {
                let root = Self::compute_merkle_root(&buffer.accumulated_leaves);
                let completed_batch = MerkleBatch {
                    batch_id: buffer.current_batch_id,
                    namespace: ns,
                    merkle_root: root,
                    parent_batch_root: buffer.parent_batch_root,
                    leaves: buffer.accumulated_leaves.clone(),
                };

                let records = std::mem::take(&mut buffer.accumulated_records);
                self.batch_archive
                    .insert(completed_batch.batch_id, completed_batch.clone());
                buffer.parent_batch_root = root;
                buffer.current_batch_id = self.global_batch_counter.fetch_add(1, Ordering::SeqCst);
                buffer.accumulated_leaves.clear();
                buffer.accumulated_tx_ids.clear();

                flushed.push((completed_batch, records));
            }
        }

        flushed
    }

    /// Proof generator: 0(log N) proof extraction for any tx_id (Archiived or Active buffer)
    pub fn generate_proof_for_tx(&self, tx_id: u128) -> Option<InclusionProof> {
        let tx_id_hex = format!("{:032x}", tx_id);

        // Check active buffer first
        for entry in self.active_buffers.iter() {
            let buffer = entry.value().read();
            if let Some(leaf_index) = buffer.accumulated_tx_ids.iter().position(|&id| id == tx_id) {
                let active_root = Self::compute_merkle_root(&buffer.accumulated_leaves);
                let proof_nodes =
                    Self::extract_sibiling_path(&buffer.accumulated_leaves, leaf_index);
                let rec = &buffer.accumulated_records[leaf_index];

                return Some(InclusionProof {
                    tx_id_hex,
                    agent_hex: hex::encode(rec.agent_id),
                    step_ordinal: rec.step_ordinal,
                    timestamp: rec.timestamp,
                    stage: rec.stage as u8,
                    namespace: rec.namespace.clone(),
                    call_sig_hex: hex::encode(rec.call_signature_hash),
                    payload_salt_hex: hex::encode(rec.payload_salt),
                    payload_hash_hex: hex::encode(rec.payload_hash),
                    leaf_index,
                    total_leaves_in_tree: buffer.accumulated_leaves.len(),
                    sibling_hashes_hex: proof_nodes.iter().map(hex::encode).collect(),
                    merkle_root_hex: hex::encode(active_root),
                    parent_batch_root_hex: hex::encode(buffer.parent_batch_root),
                    batch_id: buffer.current_batch_id,
                    is_active_buffer: true,
                });
            }
        }

        // Check crystallized batch archive
        if let Some(location) = self.tx_to_location.get(&tx_id) {
            let (batch_id, leaf_index) = *location.value();
            if let Some(batch) = self.batch_archive.get(&batch_id) {
                let proof_nodes = Self::extract_sibiling_path(&batch.leaves, leaf_index);
                return Some(InclusionProof {
                    tx_id_hex,
                    agent_hex: String::new(),
                    step_ordinal: 0,
                    timestamp: 0,
                    stage: ExecutionStage::Outcome as u8,
                    namespace: batch.namespace.clone(),
                    call_sig_hex: String::new(),
                    payload_salt_hex: String::new(),
                    payload_hash_hex: String::new(),
                    leaf_index,
                    total_leaves_in_tree: batch.leaves.len(),
                    sibling_hashes_hex: proof_nodes.iter().map(hex::encode).collect(),
                    merkle_root_hex: hex::encode(batch.merkle_root),
                    parent_batch_root_hex: hex::encode(batch.parent_batch_root),
                    batch_id,
                    is_active_buffer: false,
                });
            }
        }

        None
    }

    fn extract_sibiling_path(leaves: &[[u8; 32]], leaf_index: usize) -> Vec<[u8; 32]> {
        let mut sibling_hashes = Vec::new();
        let mut current_level = leaves.to_vec();
        let mut index = leaf_index;

        while current_level.len() > 1 {
            let mut next_level = Vec::with_capacity((current_level.len() + 1) / 2);

            for chunk in current_level.chunks(2) {
                if chunk.len() == 2 {
                    let mut hasher = Hasher::new_derive_key("raqim.merkle.v2.node");
                    hasher.update(&chunk[0]);
                    hasher.update(&chunk[1]);
                    next_level.push(hasher.finalize().into());
                } else {
                    let mut hasher = Hasher::new_derive_key("raqim.merkle.v2.node_odd");
                    hasher.update(&chunk[0]);
                    next_level.push(hasher.finalize().into());
                }
            }

            if index % 2 == 0 {
                if index + 1 < current_level.len() {
                    sibling_hashes.push(current_level[index + 1]);
                } else {
                    // Sentinel [0u8; 32] marks an unpaired odd boundary step
                    sibling_hashes.push([0u8; 32]);
                }
            } else {
                sibling_hashes.push(current_level[index - 1]);
            }

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
    ) -> Option<(MerkleBatch, Vec<ExecutionRecord>)> {
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
                merkle_root: root,
                parent_batch_root: buffer.parent_batch_root,
                leaves: buffer.accumulated_leaves.clone(),
            };

            //  Archive completed block for historical query proofs
            let batch_records = std::mem::take(&mut buffer.accumulated_records);
            self.batch_archive
                .insert(completed_batch.batch_id, completed_batch.clone());

            // Advace the State pipeline cleanly
            buffer.parent_batch_root = root;
            buffer.current_batch_id = self.global_batch_counter.fetch_add(1, SeqCst);
            buffer.accumulated_leaves.clear();
            buffer.accumulated_tx_ids.clear();

            return Some((completed_batch, batch_records));
        }

        None
    }

    /// THE PHOENIX CRASH RECOVERY COMPONENT
    pub fn hydrate_from_recovery(&self, record: &ExecutionRecord) {
        if self.tx_to_location.contains_key(&record.tx_id) {
            return; // Idempotent deduplication guard
        }

        let _ = self.ingest_leaf_internal(&record.namespace, record.leaf_hash, record.clone());
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
            for (idx, &tx_id) in snap.accumulated_tx_ids.iter().enumerate() {
                self.tx_to_location
                    .insert(tx_id, (snap.current_batch_id, idx));
            }

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

    /// Offline proof Verifier: verifies both payload commitment AND  full action envelope inclusion.
    pub fn verify_inclusion(
        payload_bytes: &[u8],
        agent_id: &[u8; 16],
        proof: &InclusionProof,
    ) -> bool {
        let Ok(tx_id) = u128::from_str_radix(proof.tx_id_hex.trim_start_matches("0x"), 16) else {
            return false;
        };
        let Ok(call_sig_vec) = hex::decode(&proof.call_sig_hex) else {
            return false;
        };
        let Ok(call_sig): Result<[u8; 32], _> = call_sig_vec.try_into() else {
            return false;
        };
        let Ok(salt_vec) = hex::decode(&proof.payload_salt_hex) else {
            return false;
        };
        let Ok(salt): Result<[u8; 16], _> = salt_vec.try_into() else {
            return false;
        };

        // verify salted pagyload commitment.
        let computed_payload_hash =
            ExecutionRecord::compute_salted_payload_hash(&salt, payload_bytes);
        if hex::encode(computed_payload_hash) != proof.payload_hash_hex {
            return false;
        }

        // Verify full envelope leaf hash and Merkle path
        let stage = ExecutionStage::from_u8(proof.stage);
        let mut current_hash = Self::compute_envelope_leaf_hash(
            tx_id,
            agent_id,
            proof.step_ordinal,
            proof.timestamp,
            stage,
            &proof.namespace,
            &call_sig,
            &computed_payload_hash,
        );

        let mut index = proof.leaf_index;
        let mut level_len = proof.total_leaves_in_tree;

        for sibling_hex in &proof.sibling_hashes_hex {
            let Ok(sibling_bytes) = hex::decode(sibling_hex) else {
                return false;
            };

            if sibling_bytes.len() != 32 {
                return false;
            }

            if index % 2 == 0 {
                if index + 1 < level_len {
                    let mut hasher = Hasher::new_derive_key("raqim.merkle.v2.node");
                    hasher.update(&current_hash);
                    hasher.update(&sibling_bytes);
                    current_hash = hasher.finalize().into()
                } else {
                    let mut hasher = Hasher::new_derive_key("raim.merkle.v2.node_odd");
                    hasher.update(&current_hash);
                    current_hash = hasher.finalize().into();
                }
            } else {
                let mut hasher = Hasher::new_derive_key("raqim.merkle.v2.node");
                hasher.update(&sibling_bytes);
                hasher.update(&current_hash);
                current_hash = hasher.finalize().into();
            }

            index /= 2;
            level_len = (level_len + 1) / 2;
        }

        hex::encode(current_hash) == proof.merkle_root_hex
    }

    /// Boot Forensic Audit: Verified Ed25519 witness sig AND merkle roots against WORM anchors.
    pub async fn execute_forensic_boot_audit(
        &self,
        witnesses: &[AnchoredRootWitness],
        witness_engine: &WormWitnessEngine,
        master_verifying_key: &VerifyingKey,
    ) -> Result<(), anyhow::Error> {
        println!(
            "[PHOENIX AUDIT] Verifying WORM witness signatures and local Merkle batch roots..."
        );

        for witness in witnesses {
            // 1. Cryptographically Verify the witness signature itself!

            let unsigned_payloaod = format!(
                "raqim.worm.v1:{}:{}:{}:{}:{}",
                witness.batch_id,
                witness.namespace,
                witness.merkle_root_hex,
                witness.parent_batch_root_hex,
                witness.timestamp
            );
            let sig_bytes: [u8; 64] = hex::decode(&witness.master_signature_hex)?
                .try_into()
                .map_err(|_| anyhow::anyhow!("Invalid witness signature length"))?;
            let sig = Signature::from_bytes(&sig_bytes);

            master_verifying_key
                .verify(unsigned_payloaod.as_bytes(), &sig)
                .map_err(|_| {
                    anyhow::anyhow!(
                        "FATAL: WORM witness header #{} failed Ed25519 signature verification!",
                        witness.batch_id
                    )
                })?;

            // 2. Verify local batch matches the signed witness root
            let mut local_valid = false;

            if let Some(local_batch) = self.batch_archive.get(&witness.batch_id) {
                if hex::encode(local_batch.merkle_root) == witness.merkle_root_hex {
                    local_valid = true;
                }
            }

            if !local_valid {
                eprintln!(
                    "[PHOENIX RED ALERT] Mismatch on Batch #{}! Restoring from WORM Witness Vault...",
                    witness.batch_id
                );

                let intact_bundle = witness_engine
                    .fetch_bundle_from_witness(witness.batch_id)
                    .await?;

                let computed_root = Self::compute_merkle_root(
                    &intact_bundle
                        .raw_logs
                        .iter()
                        .map(|r| r.leaf_hash)
                        .collect::<Vec<_>>(),
                );

                if hex::encode(computed_root) != witness.merkle_root_hex {
                    return Err(anyhow::anyhow!(
                        "FATAL: WORM witness bundle #{} failed root self-authentication!",
                        witness.batch_id
                    ));
                }

                let parent_root: [u8; 32] =
                    hex::decode(&intact_bundle.witness.parent_batch_root_hex)?
                        .try_into()
                        .unwrap_or([0u8; 32]);

                let recomputed_batch = MerkleBatch {
                    batch_id: intact_bundle.witness.batch_id,
                    namespace: intact_bundle.witness.namespace.clone(),
                    merkle_root: computed_root,
                    parent_batch_root: parent_root,
                    leaves: intact_bundle.raw_logs.iter().map(|r| r.leaf_hash).collect(),
                };

                self.batch_archive
                    .insert(recomputed_batch.batch_id, recomputed_batch);

                for (idx, rec) in intact_bundle.raw_logs.iter().enumerate() {
                    self.tx_to_location
                        .insert(rec.tx_id, (witness.batch_id, idx));
                }
            }
        }

        println!("[PHOENIX AUDIT] All historical batches verified against signed WORM witnesses.");
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
