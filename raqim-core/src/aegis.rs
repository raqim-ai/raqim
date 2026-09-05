use crate::SystemEvent;
use crate::api::UiEvent;
use dashmap::DashMap;
use ed25519_dalek::{Signature, Verifier, VerifyingKey};
use parking_lot::RwLock;
use serde::{Deserialize, Serialize};
use std::sync::atomic::AtomicU64;
use std::time::{SystemTime, UNIX_EPOCH};
use std::{collections::HashMap, sync::Arc};
use std::{eprintln, println};
use tokio::sync::broadcast::Sender;

/// The Internal Token packed inside every agent's SDK bundle
#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct CapabilityCertificate {
    pub agent_hex: String,
    pub group_name: String,
    pub expiration_timestamp: u64,
    pub master_signature: Vec<u8>, // Signed by Swarm Master Key
}

/// Atomic Token Bucket Rate Limiter
#[derive(Serialize, Deserialize, Debug)]

pub struct AtomicTokenBucket {
    pub max_tps: u64,
    pub burst_capacity: u64,
    pub tokens: AtomicU64,
    pub last_refill_nanos: AtomicU64,
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

    /// HARDENED: Atomic CAS consumption loop. Immute to underflows.
    pub fn check_and_consume(&self) -> bool {
        self.refill();

        let mut current = self.tokens.load(std::sync::atomic::Ordering::Relaxed);
        loop {
            if current == 0 {
                return false;
            }

            // Atomic CAS
            match self.tokens.compare_exchange_weak(
                current,
                current - 1,
                std::sync::atomic::Ordering::AcqRel,
                std::sync::atomic::Ordering::Relaxed,
            ) {
                Ok(_) => return true,
                Err(actual) => current = actual,
            }
        }
    }
}

#[derive(Deserialize, Debug, Clone)]
pub struct GroupPolicy {
    pub allowed_namespaces: Vec<String>,
    pub blocked_namespaces: Vec<String>,
    pub rate_limiter: Arc<AtomicTokenBucket>,
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
pub struct AegisConfigFile {
    pub groups: HashMap<String, GroupPolicyConfig>,
}

impl AegisConfigFile {
    pub fn to_group_policies(&self) -> HashMap<String, GroupPolicy> {
        let mut map = HashMap::new();
        for (group_name, cfg) in &self.groups {
            map.insert(
                group_name.clone(),
                GroupPolicy {
                    allowed_namespaces: cfg.allowed_namespaces.clone(),
                    blocked_namespaces: cfg.blocked_namespaces.clone(),
                    rate_limiter: Arc::new(AtomicTokenBucket::new(cfg.max_tps, cfg.burst_capacity)),
                },
            );
        }

        map
    }
}

pub struct AegisGateKeeper {
    pub group_policies: Arc<RwLock<HashMap<String, GroupPolicy>>>,
    pub quarantine_blocklist: DashMap<String, QuarantineRecord>,
    pub master_public_key: VerifyingKey,
    pub tx: Sender<SystemEvent>,
    pub ui_tx: Sender<UiEvent>,
}

impl AegisGateKeeper {
    pub fn new(
        initial_policies: HashMap<String, GroupPolicy>,
        master_pub_bytes: &[u8; 32],
        tx: Sender<SystemEvent>,
        ui_tx: Sender<UiEvent>,
    ) -> Self {
        let master_public_key = VerifyingKey::from_bytes(master_pub_bytes)
            .expect("FATAL: Failed to parse master public key");

        Self {
            quarantine_blocklist: DashMap::new(),
            group_policies: Arc::new(RwLock::new(initial_policies)),
            master_public_key,
            tx,
            ui_tx,
        }
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

    /// Locks down the agent globally across the OS
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

        // Lock the agent down at network layer instantly.
        self.quarantine_blocklist
            .insert(agent_hex.to_string(), record.clone());

        // Shout into the event bus
        let _ = self.tx.send(SystemEvent::GlobalQuarantineSync {
            record: record.clone(),
        });

        // Fire the durable WAL log
        let _ = self.tx.send(SystemEvent::AegisInterdiction {
            agent_id: agent_hex.to_string(),
            attempted_path: target.to_string(),
            rule_broken: v_type.to_string(),
            payload: reason.to_string(),
        });

        // Fire the SSE alert directly to the React Terminal
        let _ = self.ui_tx.send(UiEvent::AegisAlert {
            record: record.clone(),
        });

        let _ = eprintln!(
            "\n[AEGIS RED ALERT] Unauthorized access attempts by {} on path: {}, Violation Type: {}, Reason: {} ",
            agent_hex, target, v_type, reason
        );
    }

    /// Remote Ingestion: Triggered when a foreign node broadcasts a quarantine over Zenoh
    pub fn assimilate_remote_quarantine(&self, record: QuarantineRecord) {
        // prevent redundant processing if already blocklisted
        if self.quarantine_blocklist.contains_key(&record.agent_hex) {
            return;
        }

        //  Mutate local firewall blocklist instantly.
        self.quarantine_blocklist
            .insert(record.agent_hex.clone(), record.clone());

        // Trigger local security breach alerts
        let _ = self.tx.send(SystemEvent::SecurityBreach {
            agent_id: record.agent_hex.clone(),
            reason: format!(
                "Global Network Quarantine: {}",
                record.violation_type.clone()
            ),
            culprit_text: record.payload_preview.clone(),
        });

        let _ = self.ui_tx.send(UiEvent::AegisAlert {
            record: record.clone(),
        });

        eprintln!(
            "[AEGIS MESH INTERDICTION] Remote quarantine assimilated from network for agent {}. Reason: {} ",
            record.agent_hex, record.violation_type
        );
    }

    /// Validates the cryptographic token structure and executes signature audit at the gateway.
    // The ultra-fast packet audit (Called once per packet)
    pub fn authorize_packet_fast(
        &self,
        agent_hex: &str,
        group_name: &str,
        agent_pub_bytes: &[u8; 32],
        payload: &[u8],
        packet_sig_bytes: &[u8; 64],
        intent_path: &str,
        packet_timestamp: i64,
    ) -> Result<(), anyhow::Error> {
        // Freshness Window Check
        let current_ts = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs() as i64;

        if (current_ts - packet_timestamp).abs() > 30 {
            self.trigger_quarantine(
                agent_hex,
                intent_path,
                "REPLAY_ATTACK",
                "Packet timestamp expired (drift > 30s)",
            );

            return Err(anyhow::anyhow!(
                "Security Violation: State packet rejected (Anti-replay)"
            ));
        };

        //  AUTHENTICITY VERIFICATION: Verify payload integrity against individual Agent Key.
        let agent_verifying_key = VerifyingKey::from_bytes(agent_pub_bytes)?;
        let packet_sig = Signature::from_bytes(packet_sig_bytes);
        if agent_verifying_key.verify(payload, &packet_sig).is_err() {
            self.trigger_quarantine(
                agent_hex,
                &intent_path,
                "CRYPTO_SPOOF",
                "Invalid Agent Frame Signature",
            );
            return Err(anyhow::anyhow!(
                "Integrity Audit Failure: Mismatched Agent Handshake "
            ));
        }

        // Rate limiting & Dos interdiction (Atomic bucket token)
        let policies_guard = self.group_policies.read();
        let live_policy = policies_guard.get(group_name).ok_or_else(|| {
            anyhow::anyhow!(
                "Group Policy mapping '{}' not defined inside active aegis.toml ",
                group_name
            )
        })?;

        if !live_policy.rate_limiter.check_and_consume() {
            self.trigger_quarantine(
                agent_hex,
                intent_path,
                "RATE_LIMIT_EXCEDED",
                &format!("Agent exceeded group '{}' max TPS quota", group_name),
            );

            return Err(anyhow::anyhow!(
                "Acess Denied: Group Rate Limit Exceeded (DoS Interdiction)"
            ));
        }

        for blocked in &live_policy.blocked_namespaces {
            let match_found = if blocked.ends_with("*") {
                intent_path.starts_with(&blocked[..blocked.len() - 1])
            } else {
                intent_path == blocked
            };

            if match_found {
                self.trigger_quarantine(
                    agent_hex,
                    intent_path,
                    "NAMESPACE_BREACH",
                    "Atempted interaction inside expicitely blocked domain",
                );
                return Err(anyhow::anyhow!(
                    "Access Denied: Namespace explicitely blocked"
                ));
            }
        }

        for allowed in &live_policy.allowed_namespaces {
            let match_found = if allowed.ends_with("*") {
                intent_path.starts_with(&allowed[..allowed.len() - 1])
            } else {
                allowed == intent_path
            };

            if match_found {
                return Ok(());
            }
        }

        // Default Deny Fallback
        self.trigger_quarantine(
            agent_hex,
            intent_path,
            "NAMESPACE_BREACH",
            "No explicit allowance match within token permissions",
        );
        Err(anyhow::anyhow!(
            "Access Denied: Default Deny Policy Tripped"
        ))
    }

    /// The heavy handshake: Validates Master Certificate AND binds it to the packet's public key
    pub fn verify_session_lineage(
        &self,
        cert_bytes: &[u8],
        agent_pub_bytes: &[u8; 32],
    ) -> Result<(String, String), anyhow::Error> {
        // Unpack the certificate token
        let cert: CapabilityCertificate = postcard::from_bytes(cert_bytes)
            .map_err(|_| anyhow::anyhow!("Malformed Cryptographic Certificate Token"))?;

        // Cryptographic binding: derive blake3 agent_hex directly from the incoming packet's pub key
        let mut hasher = blake3::Hasher::new_derive_key("raqim.agent.v1.identity");
        hasher.update(agent_pub_bytes);
        let mut derived_bytes = [0u8; 16];
        hasher.finalize_xof().fill(&mut derived_bytes);
        let derived_agent_hex = hex::encode(derived_bytes);

        // Assert that the Certificate ID matches the key that actually signed the packet
        if cert.agent_hex != derived_agent_hex {
            self.trigger_quarantine(
                &derived_agent_hex,
                "Handshake",
                "CONFUSED_DEPUTY_SPOOF",
                "Public key does not match CapabilityCertificate identity",
            );

            return Err(anyhow::anyhow!(
                "Security Violation: Certificate identity mismatch with signing key "
            ));
        }

        // 2. Short-circuit check if the agent is actively quarantined
        if self.is_quarantined(&cert.agent_hex) {
            return Err(anyhow::anyhow!(
                " Agent is expicitely locked down by firewall "
            ));
        }

        // 3. Audit Certifiicate Expiration
        let current_ts = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_secs();
        if current_ts > cert.expiration_timestamp {
            return Err(anyhow::anyhow!(" Capability Certificate has expired "));
        }

        // 4. LINEAGE VERIFICATION: Prove token validity against the Master Swarm Key
        let mut cert_unsigned_payload = cert.clone();
        cert_unsigned_payload.master_signature = Vec::new();
        let serialized_raw = postcard::to_allocvec(&cert_unsigned_payload)?;

        let master_sig_bytes: &[u8; 64] = cert
            .master_signature
            .as_slice()
            .try_into()
            .map_err(|_| anyhow::anyhow!("Invalid Master Signature block lengnth"))?;

        let master_sig = Signature::from_bytes(master_sig_bytes);
        if self
            .master_public_key
            .verify(&serialized_raw, &master_sig)
            .is_err()
        {
            self.trigger_quarantine(
                &cert.agent_hex,
                "Handshake",
                "CRYPTO_SPOOF",
                "Forged Swarm Lineage Token",
            );
            return Err(anyhow::anyhow!(
                "Lineage Audit Failure: Forged Master Signature"
            ));
        }

        Ok((cert.agent_hex, cert.group_name))
    }
}
