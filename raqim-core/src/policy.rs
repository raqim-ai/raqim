use crate::SystemEvent;
use dashmap::DashMap;
use ed25519_dalek::{Signature, Verifier, VerifyingKey};
use parking_lot::RwLock;
use serde::{Deserialize, Serialize};
use std::sync::atomic::{AtomicU32, AtomicU64, Ordering};
use std::time::{SystemTime, UNIX_EPOCH};
use std::{collections::HashMap, sync::Arc};
use std::{eprintln, format, println};
use tokio::sync::broadcast::Sender;

/// Number of consecutive rate-limit rejections before an afent is deemed a runaway flood and placed into Quarantine.
pub const MAX_CONSECUTIVE_RATE_STRIKES: u32 = 50;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PolicyError {
    Unauthenticated(String),
    Quarantined(String),
    RateLimited(String),
    ForbiddenNamespace(String),
}

impl std::fmt::Display for PolicyError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            PolicyError::Unauthenticated(m) => write!(f, "Unauthenticated: {}", m),
            PolicyError::Quarantined(m) => write!(f, "Quarantined: {}", m),
            PolicyError::RateLimited(m) => write!(f, "Rate Limit Exceeded: {}", m),
            PolicyError::ForbiddenNamespace(m) => write!(f, "Policy Interdiction: {}", m),
        }
    }
}

impl std::error::Error for PolicyError {}

/// Signed capability passport binding an agent to a security group
#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct CapabilityCertificate {
    pub agent_hex: String,
    pub group_name: String,
    pub expiration_timestamp: u64,
    pub master_signature: Vec<u8>,
}

/// Lock-Free Hardware CAS Token Bucket with consecutive flood strike counter.
#[derive(Debug)]

pub struct AtomicTokenBucket {
    pub max_tps: u64,
    pub burst_capacity: u64,
    pub tokens: AtomicU64,
    pub last_refill_nanos: AtomicU64,
    pub consecutive_violations: AtomicU32,
}

impl AtomicTokenBucket {
    pub fn new(max_tps: u64, burst_capacity: u64) -> Self {
        let now_nanos = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_nanos() as u64;

        Self {
            max_tps,
            burst_capacity,
            tokens: AtomicU64::new(burst_capacity),
            last_refill_nanos: AtomicU64::new(now_nanos),
            consecutive_violations: AtomicU32::new(0),
        }
    }

    /// Atomically refills tokens based on elapsed nanoseconds
    pub fn refill(&self) {
        let now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_nanos() as u64;

        let mut last = self
            .last_refill_nanos
            .load(std::sync::atomic::Ordering::Relaxed);
        loop {
            if now <= last {
                // Monotoic protection
                break;
            }

            let elapsed_nanos = now - last;

            // Tokens to add = (elapsed_nanos * max_tps) / 1,000,000,000
            let tokens_to_add =
                (elapsed_nanos as u128 * self.max_tps as u128 / 1_000_000_000) as u64;

            if tokens_to_add == 0 {
                break;
            }

            // update last_refill_nanos atomically
            match self.last_refill_nanos.compare_exchange_weak(
                last,
                now,
                std::sync::atomic::Ordering::AcqRel,
                std::sync::atomic::Ordering::Relaxed,
            ) {
                Ok(_) => {
                    // Refill timestamp won; update token counter safely
                    let mut curr_tokens = self.tokens.load(std::sync::atomic::Ordering::Relaxed);
                    loop {
                        let new_total = (curr_tokens + tokens_to_add).min(self.burst_capacity);
                        match self.tokens.compare_exchange_weak(
                            curr_tokens,
                            new_total,
                            std::sync::atomic::Ordering::Release,
                            std::sync::atomic::Ordering::Relaxed,
                        ) {
                            Ok(_) => break,
                            Err(actual) => curr_tokens = actual,
                        }
                    }
                    break;
                }
                Err(actual) => last = actual,
            }
        }
    }

    /// Attempts to consome 1 token atomically.
    /// - On success: resets `consecutvive_violations` to 0 and returns Ok(())
    /// - On exhaustion: increments `consecutive_violations` and returns `Err(strike_count)`
    pub fn check_and_consume(&self) -> Result<(), u32> {
        self.refill();

        let mut current = self.tokens.load(std::sync::atomic::Ordering::Relaxed);
        loop {
            if current == 0 {
                let strikes = self.consecutive_violations.fetch_add(1, Ordering::AcqRel);
                return Err(strikes);
            }

            // Atomic CAS
            match self.tokens.compare_exchange_weak(
                current,
                current - 1,
                std::sync::atomic::Ordering::AcqRel,
                std::sync::atomic::Ordering::Relaxed,
            ) {
                Ok(_) => {
                    self.consecutive_violations.store(0, Ordering::Relaxed);
                    return Ok(());
                }
                Err(actual) => current = actual,
            }
        }
    }
}

#[derive(Debug, Clone)]
pub struct GroupPolicy {
    pub allowed_namespaces: Vec<String>,
    pub blocked_namespaces: Vec<String>,
    pub max_tps: u64,
    pub burst_capacity: u64,
}

#[derive(Clone, Debug, Serialize, Deserialize, rkyv::Archive, rkyv::Serialize)]
pub struct QuarantineRecord {
    pub agent_hex: String,
    pub violation_type: String,
    pub attemped_path: String,
    pub payload_preview: String,
    pub timestamp: u64,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct GroupPolicyConfig {
    pub allowed_namespaces: Vec<String>,
    pub blocked_namespaces: Vec<String>,
    pub max_tps: u64,
    pub burst_capacity: u64,
}

/// Deserialization schema for aegis.toml
#[derive(Deserialize, Debug, Clone, Serialize)]
pub struct PolicyConfigFile {
    pub groups: HashMap<String, GroupPolicyConfig>,
}

impl PolicyConfigFile {
    pub fn to_group_policies(&self) -> HashMap<String, GroupPolicy> {
        let mut map = HashMap::new();
        for (group_name, cfg) in &self.groups {
            map.insert(
                group_name.clone(),
                GroupPolicy {
                    allowed_namespaces: cfg.allowed_namespaces.clone(),
                    blocked_namespaces: cfg.blocked_namespaces.clone(),
                    max_tps: cfg.max_tps,
                    burst_capacity: cfg.burst_capacity,
                },
            );
        }

        map
    }
}

pub struct PolicyEngine {
    pub group_policies: Arc<RwLock<HashMap<String, GroupPolicy>>>,
    pub agent_rate_limiters: DashMap<String, Arc<AtomicTokenBucket>>,
    pub quarantine_blocklist: DashMap<String, QuarantineRecord>,
    pub master_public_key: VerifyingKey,
    pub tx: Sender<SystemEvent>,
    pub control_journal_path: String,
}

impl PolicyEngine {
    pub fn new(
        initial_policies: HashMap<String, GroupPolicy>,
        master_pub_bytes: &[u8; 32],
        tx: Sender<SystemEvent>,
        control_journal_path: String,
    ) -> Self {
        let master_public_key = VerifyingKey::from_bytes(master_pub_bytes)
            .expect("FATAL: Failed to parse master public key");

        Self {
            quarantine_blocklist: DashMap::new(),
            agent_rate_limiters: DashMap::new(),
            group_policies: Arc::new(RwLock::new(initial_policies)),
            master_public_key,
            tx,
            control_journal_path,
        }
    }

    /// Persists a quarantine record to durable storage via control journal
    fn persist_quarantine_to_journal(
        &self,
        record: &QuarantineRecord,
    ) -> Result<(), anyhow::Error> {
        crate::checkpoint::CheckpointEngine::append_control_mutation(
            std::path::Path::new(&self.control_journal_path),
            &crate::checkpoint::ControlMutation::Quarantine(record.clone()),
        )
    }

    /// Exports active quarantined records for checkpointing
    pub fn export_quarantines(&self) -> Vec<QuarantineRecord> {
        self.quarantine_blocklist
            .iter()
            .map(|e| e.value().clone())
            .collect()
    }

    /// Hydrates quarantine records into RAM from checkpoint
    pub fn hydrate_quarantines(&self, records: Vec<QuarantineRecord>) {
        for record in records {
            self.quarantine_blocklist
                .insert(record.agent_hex.clone(), record);
        }
    }

    // Hydrate QuarantineRecord RAM from legacy disk fallback
    pub fn hydrate_quarantine_from_disk(&self) -> usize {
        let path = std::path::Path::new("./vault/quarantine.json");
        if !path.exists() {
            return 0;
        }

        let records: Vec<QuarantineRecord> = std::fs::read_to_string(path)
            .ok()
            .and_then(|s| serde_json::from_str(&s).ok())
            .unwrap_or_default();
        let count = records.len();

        for record in records {
            self.quarantine_blocklist
                .insert(record.agent_hex.clone(), record);
        }

        if count > 0 {
            println!(
                "[AEGIS BOOT] Hydrated {} active quarantined agents from durable storage.",
                count
            );
        }

        count
    }

    /// Hot-reloaded API: Override memory policy maps when file changes occur on disk
    pub fn reload_policies(&self, new_policies: HashMap<String, GroupPolicy>) {
        let mut guard = self.group_policies.write();
        *guard = new_policies;

        println!(
            "[AEGIS FIREWALL] Successfully hot-reloaded policy updates from disk kernel watchers."
        );
    }

    pub fn is_quarantined(&self, agent_hex: &str) -> bool {
        self.quarantine_blocklist.contains_key(agent_hex)
    }

    /// Place an agent into quarantine and durably journals the mutation
    pub fn trigger_quarantine(&self, agent_hex: &str, target: &str, v_type: &str, reason: &str) {
        let timestamp = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_secs();

        let record = QuarantineRecord {
            agent_hex: agent_hex.to_string(),
            violation_type: v_type.to_string(),
            attemped_path: target.to_string(),
            payload_preview: reason.to_string(),
            timestamp,
        };

        self.quarantine_blocklist
            .insert(agent_hex.to_string(), record.clone());

        if let Err(e) = self.persist_quarantine_to_journal(&record) {
            eprintln!(
                "[POLICY CRITICAL] Failed to persist quarantine to journal '{}': {} ",
                self.control_journal_path, e
            );
        }

        let _ = self.tx.send(SystemEvent::QuarantineTriggered {
            record: record.clone(),
        });

        // Fire the durable WAL log
        let _ = self.tx.send(SystemEvent::PolicyInterdiction {
            agent_id: agent_hex.to_string(),
            attempted_path: target.to_string(),
            rule_broken: v_type.to_string(),
            payload: reason.to_string(),
        });

        eprintln!(
            "\n[POLICY INTERDICTION] Quartantined agent {} on path: '{}' : {}, ({})",
            agent_hex, target, v_type, reason
        );
    }

    /// Evaluates rate limits (Two-Tier) and namespace ACL for ab already-authenticated agent
    pub fn authorize_namespace_only(
        &self,
        agent_hex: &str,
        group_name: &str,
        namespace: &str,
    ) -> Result<(), PolicyError> {
        if self.is_quarantined(agent_hex) {
            return Err(PolicyError::Quarantined(format!(
                "Agent '{}' is locked in quarantine blocklist",
                agent_hex
            )));
        }

        let policies_guard = self.group_policies.read();
        let live_policy = policies_guard.get(group_name).ok_or_else(|| {
            PolicyError::ForbiddenNamespace(format!("Undefined policy group '{}'", group_name))
        })?;

        let agent_bucket = self
            .agent_rate_limiters
            .entry(agent_hex.to_string())
            .or_insert_with(|| {
                Arc::new(AtomicTokenBucket::new(
                    live_policy.max_tps,
                    live_policy.burst_capacity,
                ))
            })
            .value()
            .clone();

        if let Err(strikes) = agent_bucket.check_and_consume() {
            let max_tps = live_policy.max_tps;
            drop(policies_guard);
            if strikes >= MAX_CONSECUTIVE_RATE_STRIKES {
                self.trigger_quarantine(
                    agent_hex,
                    namespace,
                    "SUSTAINED_RATE_FLOOD",
                    &format!(
                        "Agent exceeded rate limit {} consecutive times (runaway flood)",
                        strikes
                    ),
                );
                return Err(PolicyError::Quarantined(format!(
                    "Agent '{}' quarantined after {} consecutive rate-limit violations",
                    agent_hex, strikes
                )));
            }
            return Err(PolicyError::RateLimited(format!(
                "Agent '{}' exceeded rate limit ({} TPS, strike {}/{})",
                agent_hex, max_tps, strikes, MAX_CONSECUTIVE_RATE_STRIKES
            )));
        }

        // Strip `phantom_` prefix so counterfactual replay branches cannot evade namespace rules
        let clean_ns = namespace.strip_prefix("phantom_").unwrap_or(namespace);

        for blocked in &live_policy.blocked_namespaces {
            let match_found = if let Some(prefix) = blocked.strip_suffix('*') {
                clean_ns.starts_with(prefix)
            } else {
                clean_ns == blocked
            };

            if match_found {
                drop(policies_guard);
                self.trigger_quarantine(
                    agent_hex,
                    namespace,
                    "NAMESPACE_BREACH",
                    "Attempted interaction inside explicitly blocked namespace",
                );
                return Err(PolicyError::ForbiddenNamespace(format!(
                    "Namespace '{}' is explicitly blocked",
                    namespace
                )));
            }
        }

        for allowed in &live_policy.allowed_namespaces {
            let match_found = if let Some(prefix) = allowed.strip_suffix('*') {
                clean_ns.starts_with(prefix)
            } else {
                allowed == clean_ns
            };

            if match_found {
                return Ok(());
            }
        }

        drop(policies_guard);
        self.trigger_quarantine(
            agent_hex,
            namespace,
            "NAMESPACE_BREACH",
            "No matching allowance rule in policy (Default Deny)",
        );
        Err(PolicyError::ForbiddenNamespace(
            "Default Deny Policy Tripped".into(),
        ))
    }

    /// Full fast-path auth:
    pub fn authorize_packet_fast(
        &self,
        agent_hex: &str,
        group_name: &str,
        agent_pub_bytes: &[u8; 32],
        payload: &[u8],
        packet_sig_bytes: &[u8; 64],
        namespace: &str,
        packet_timestamp: i64,
    ) -> Result<(), PolicyError> {
        let agent_verifying_key = VerifyingKey::from_bytes(agent_pub_bytes)
            .map_err(|_| PolicyError::Unauthenticated("Invalid Ed25519 public key".into()))?;

        let packet_sig = Signature::from_bytes(packet_sig_bytes);
        if agent_verifying_key.verify(payload, &packet_sig).is_err() {
            return Err(PolicyError::Unauthenticated(
                "Invalid Ed25519 packet signature".into(),
            ));
        }

        let current_ts = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs() as i64;
        if (current_ts - packet_timestamp).abs() > 30 {
            return Err(PolicyError::Unauthenticated(
                "Packet timestamp outside 30s anti-replay window".into(),
            ));
        }

        self.authorize_namespace_only(agent_hex, group_name, namespace)
    }

    /// Validates MAster CA certificate AND binds it to `agent_pub_bytes`. Never quarantines on unauthenticated/forged certs.
    pub fn verify_session_lineage(
        &self,
        cert_bytes: &[u8],
        agent_pub_bytes: &[u8; 32],
    ) -> Result<(String, String), PolicyError> {
        // Unpack the certificate token
        let cert: CapabilityCertificate = postcard::from_bytes(cert_bytes).map_err(|_| {
            PolicyError::Unauthenticated("Malformed Cryptographic Certificate Token".into())
        })?;

        // Cryptographic binding: derive blake3 agent_hex directly from the incoming packet's pub key
        let mut hasher = blake3::Hasher::new_derive_key("raqim.agent.v1.identity");
        hasher.update(agent_pub_bytes);
        let mut derived_bytes = [0u8; 16];
        hasher.finalize_xof().fill(&mut derived_bytes);
        let derived_agent_hex = hex::encode(derived_bytes);

        // Assert that the Certificate ID matches the key that actually signed the packet
        if cert.agent_hex != derived_agent_hex {
            return Err(PolicyError::Unauthenticated(
                "Public key does not match CapabilityCertificate agent_hex".into(),
            ));
        }

        // 3. Audit Certifiicate Expiration
        let current_ts = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_secs();
        if current_ts > cert.expiration_timestamp {
            return Err(PolicyError::Unauthenticated(
                " CapabilityCertificate has expired ".into(),
            ));
        }

        // 4. LINEAGE VERIFICATION: Prove token validity against the Master Swarm Key
        let mut cert_unsigned_payload = cert.clone();
        cert_unsigned_payload.master_signature = Vec::new();
        let serialized_raw = postcard::to_allocvec(&cert_unsigned_payload)
            .map_err(|_| PolicyError::Unauthenticated("Certificate serializion error".into()))?;

        let master_sig_bytes: &[u8; 64] =
            cert.master_signature.as_slice().try_into().map_err(|_| {
                PolicyError::Unauthenticated("Invalid Master Signature block lengnth".into())
            })?;

        let master_sig = Signature::from_bytes(master_sig_bytes);
        if self
            .master_public_key
            .verify(&serialized_raw, &master_sig)
            .is_err()
        {
            return Err(PolicyError::Unauthenticated(
                "Forged Master CA signature on CapabilityCertificate".into(),
            ));
        }

        if self.is_quarantined(&cert.agent_hex) {
            return Err(PolicyError::Quarantined(format!(
                "Agent '{}' is locked in quarantine blocklist ",
                cert.agent_hex
            )));
        }

        Ok((cert.agent_hex, cert.group_name))
    }
}
