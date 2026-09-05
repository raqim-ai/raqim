use std::sync::Arc;
use std::time::{SystemTime, UNIX_EPOCH};
use std::{eprintln, format, println};

use crate::axon::AxonGateKeeper;
use crate::state::SwarmStateRegistry;
use crate::{A2AEnvelope, OpLog, SystemEvent};
use rkyv::{Archive, to_bytes};
use tokio::sync::broadcast::Sender;
use tokio::sync::mpsc;
use zenoh::Session;

use crate::aegis::{AegisGateKeeper, QuarantineRecord};
use tokio::time::{Duration, timeout};

pub struct GlobalNetworkBridge {
    session: Arc<Session>,
    workspace_prefix: String,
    aegis: Arc<AegisGateKeeper>,
    pub os_node_id: String,
    egress_tx: mpsc::Sender<Vec<u8>>,
}

impl GlobalNetworkBridge {
    /// Bootstraps the modern Zenoh P2P Node
    pub async fn new(
        tenant_id: &str,
        swarm_name: &str,
        aegis: Arc<AegisGateKeeper>,
        os_node_id: String,
    ) -> Self {
        println!("Bismillah. Initialializing Zenoh Global Network Bridge with Dynamic Atomic...");

        let mut config = zenoh::Config::default();

        config
            .insert_json5("scouting/multicast/enabled", "true")
            .unwrap();

        let session = zenoh::open(config.clone())
            .await
            .expect("Failed to start zenoh");
        let workspace_prefix = format!("raqim/{}/{}", tenant_id, swarm_name);

        // Bounded Egress funnel
        let (egress_tx, mut egress_rx) = mpsc::channel::<Vec<u8>>(100_000);

        let session_clone = session.clone();
        let topic_clone = format!("{}/thoughts/{}", workspace_prefix.clone(), os_node_id);

        // Dynamic wan state tracker inside the background egress task

        tokio::spawn(async move {
            println!(
                "[NETWORK CORE] Zenoh Egress Funnel active on topic: {} ",
                &topic_clone
            );

            while let Some(bytes) = egress_rx.recv().await {
                let _ = session_clone.put(&topic_clone, bytes).await;
            }
        });

        Self {
            session: Arc::new(session),
            workspace_prefix,
            aegis,
            os_node_id,
            egress_tx,
        }
    }

    /// Takes a locally verfied Oplog and broadcasts it to the global swarm
    pub async fn broadcast_to_world(&self, log: &OpLog) {
        let bytes = to_bytes::<rkyv::rancor::Error>(log)
            .expect("Zero-copy serialization failed")
            .into_vec();

        // Applies healthy async backprpessure if WAN is slow, without spawning tokio task.
        let _ = self.egress_tx.send(bytes).await;
    }

    /// Asks a a question to the swarm. Returns the answer
    pub async fn execute_a2a_rpc(
        &self,
        envelope: A2AEnvelope,
        aegis: Arc<AegisGateKeeper>,
    ) -> Result<(Vec<u8>, String), anyhow::Error> {
        let sender_hex = hex::encode(envelope.sender_id.clone());

        // Verify sender sesssion lineage certificate
        let (agent_hex, group_name) = match aegis.verify_session_lineage(
            &envelope.sender_capability_cert.as_slice(),
            &envelope.sender_public_key,
        ) {
            Ok((agent, group)) => (agent, group),
            Err(e) => {
                eprintln!(
                    "[AEGIS NETWORK INTERDICTION] Dropped Malicious A2A RPC query line. Reason: {}",
                    e
                );

                return Err(anyhow::anyhow!(""));
            }
        };

        // Enforce fast Aegis Packet Authorization & Anti-Replay Timestamp check
        if let Err(e) = aegis.authorize_packet_fast(
            &agent_hex,
            &group_name,
            &envelope.sender_public_key,
            &envelope.payload,
            &envelope.signature,
            &envelope.target_capability,
            envelope.timestamp,
        ) {
            return Err(anyhow::anyhow!(
                "[AEGIS INTERDICTION]: A2A Transmission Violation: {} ",
                e
            ));
        }

        // 2. Zero-Copy Serializarion the Envelope for Zenoh Transmission
        let bytes = rkyv::to_bytes::<rkyv::rancor::Error>(&envelope)
            .unwrap()
            .into_vec();

        let key_expr = format!(
            "{}/a2a/{}",
            self.workspace_prefix, envelope.target_capability
        );

        // 3. Zenoh GET request (The RPC Broadcast )
        // We broadcast the question and wait for the authoritative answer to reply.
        let replies = self
            .session
            .get(&key_expr)
            .target(zenoh::query::QueryTarget::All)
            .payload(bytes)
            .await
            .map_err(|e| anyhow::anyhow!("Zenoh query dispatch failed: {}", e))?;

        let reply_future = replies.recv_async();

        // 4. Await the response from the target agent
        if let Ok(Ok(reply)) = timeout(Duration::from_secs(15), reply_future).await {
            if let Ok(sample) = reply.result() {
                // Return the answer bytes back to the caller
                let res_bytes: Vec<u8> = sample.payload().to_bytes().to_vec();

                // Attempt to parse the pythons SDK's envelope to extract the true responder and answer
                if let Ok(json_val) = serde_json::from_slice::<serde_json::Value>(&res_bytes) {
                    let actual_responder = json_val["responder_hex"]
                        .as_str()
                        .unwrap_or(&sender_hex)
                        .to_string();

                    // Reselialize the answer bytes to send back to the original caller
                    let clean_answer = if let Some(ans_str) = json_val["answer"].as_str() {
                        ans_str.as_bytes().to_vec()
                    } else if let Some(ans_arr) = json_val["answer"].as_array() {
                        ans_arr
                            .iter()
                            .filter_map(|v| v.as_u64().map(|b| b as u8))
                            .collect()
                    } else {
                        res_bytes.clone()
                    };

                    return Ok((clean_answer, actual_responder));
                }

                // Fallback if the payload was shitly formatted
                return Ok((res_bytes, sender_hex.clone()));
            }
        }

        Err(anyhow::anyhow!(
            "A2A Timeout: No agent responded to capability {}",
            envelope.target_capability
        ))
    }

    /// Broadcasts local quarantine to the global swarm over Zenoh
    pub async fn broadcast_quarantine_sync(&self, record: QuarantineRecord) {
        let key_expr = format!("{}/system/quarantine", self.workspace_prefix);
        let bytes = postcard::to_allocvec(&record).unwrap();
        if let Err(e) = self.session.put(key_expr, bytes).await {
            eprintln!(
                "[NETWORK WARN] Failed to broadcast quarantine record: {} ",
                e
            );
        }
    }

    /// Listens for global quarantine signals broadcast by peer nodes over zenoh
    pub async fn listen_for_global_quarantine(&self, aegis: Arc<AegisGateKeeper>) {
        let key_exp = format!("{}/system/quarantine", self.workspace_prefix);
        let session_clone = self.session.clone();

        println!(
            "[NETWORK CORE] Aegis Global Quarantine subscriber active on: {} ",
            key_exp.clone()
        );

        tokio::spawn(async move {
            let subscriber = match session_clone.declare_subscriber(&key_exp).await {
                Ok(sub) => sub,
                Err(e) => {
                    eprintln!(
                        "[NETWORK FATAL] Failed to declare quarantine subscriber: {}",
                        e
                    );
                    return;
                }
            };

            while let Ok(sample) = subscriber.recv_async().await {
                let payload_bytes = sample.payload().to_bytes();

                // Deserialize incoming quarantine record
                if let Ok(record) = postcard::from_bytes::<QuarantineRecord>(&payload_bytes) {
                    // Assimilate directly into local aegis blocklist
                    aegis.assimilate_remote_quarantine(record);
                } else {
                    eprintln!(
                        "[NETWORK WARN] Received malformed QuarantineRecord on system channel "
                    );
                }
            }
        });
    }

    /// Listens for foreign thoughts from the global network using a wildcard, dropping echoes from outselves
    pub async fn listen_for_foreign_thoughts(
        &self,
        brain_registry: Arc<SwarmStateRegistry>,
        axon: Arc<AxonGateKeeper>,
        tx: Sender<SystemEvent>,
    ) {
        // We subscribe to a wildcard to catch thoughts from ALL other nodes on the planet
        let key_expr = format!("{}/thoughts/*", self.workspace_prefix);
        let session_clone = self.session.clone();
        let my_node_id = self.os_node_id.clone();

        println!(
            " [NETWORK CORE] Listening for global swarm synchronization on: {} ...",
            key_expr
        );
        tokio::spawn(async move {
            let subscriber = session_clone.declare_subscriber(key_expr).await.unwrap();

            while let Ok(sample) = subscriber.recv_async().await {
                // Extract the sender node id from the topic path
                let topic_str = sample.key_expr().as_str();
                let sender_node_id = topic_str.split("/").last().unwrap_or("");

                // The Echo filter: If this packet came from our own code, drop it instantly.
                if sender_node_id == my_node_id {
                    continue;
                }

                let payload_bytes = sample.payload().to_bytes();

                // 2. We cast pointer directly over ZENOH network buffer!
                let archived_log =
                    match rkyv::access::<<OpLog as rkyv::Archive>::Archived, rkyv::rancor::Error>(
                        &payload_bytes,
                    ) {
                        Ok(valid_archive) => valid_archive,
                        Err(e) => {
                            eprintln!(
                                "[AEGIS] Packet Dropped. Malformed memory layout (OpLog): {}",
                                e
                            );
                            continue;
                        }
                    };

                // Cryptographic verification on Raw pounter
                if axon.verify_foreign_thoughts(archived_log) {
                    let target_namespace = archived_log.state.namespace.as_str();

                    // Retreive or spin up highly isolated, independent, Loro document shard.
                    let target_brain = brain_registry.get_or_create_brain(target_namespace);

                    if let Err(e) =
                        target_brain.assimilate_foreign_thought(archived_log.delta.as_slice())
                    {
                        eprintln!(
                            "[CRDT SHARD ERROR]: Shard '{}' assimilation failed: {} ",
                            target_namespace, e
                        );
                    }
                } else {
                    eprintln!("SECURITY BREACH: Forged thought detected on network. Dropping.");
                    let _ = tx.send(SystemEvent::SecurityBreach {
                        agent_id: hex::encode(&archived_log.agent_id.as_slice()),
                        reason: "Forged thought detected on global network - Markle Hash Mismatch "
                            .to_string(),
                        culprit_text: archived_log.state.text.as_str().to_string(),
                    });
                }
            }
        });
    }

    /// Registers a local capability listener and routes queries to the handler
    pub async fn register_agent_capability<F>(&self, capability_path: &str, mut response_handler: F)
    where
        F: FnMut(&[u8]) -> Vec<u8> + Send + 'static,
    {
        let key_expr = format!("{}/a2a/{}", self.workspace_prefix, capability_path);
        let session = self.session.clone();
        let aegis = self.aegis.clone();
        tokio::spawn(async move {
            // A Queryable tells the global network: "I can answer questions for this topic"
            let queryable = match session.declare_queryable(&key_expr).await {
                Ok(q) => q,
                Err(e) => {
                    eprintln!(
                        "[A2A FATAL] Failed to declare queryable on {}: {} ",
                        key_expr, e
                    );
                    return;
                }
            };

            println!("[A2A] Capability Registered: Listening on {} ", key_expr);

            while let Ok(query) = queryable.recv_async().await {
                let payload_bytes = match query.payload() {
                    Some(p) => p.to_bytes().to_vec(),
                    None => continue,
                };

                let archievd_envelope =
                    match rkyv::access::<<A2AEnvelope as Archive>::Archived, rkyv::rancor::Error>(
                        &payload_bytes,
                    ) {
                        Ok(env) => env,
                        Err(e) => {
                            eprintln!(
                                "[A2A WARN] Malformed incoming A2AEnvelope memory layout: {} ",
                                e
                            );
                            continue;
                        }
                    };

                // Extract the raw question bytes
                let question_payload = archievd_envelope.payload.as_slice();

                let mut packet_signature = [0u8; 64];
                if archievd_envelope.signature.len() == 64 {
                    packet_signature.copy_from_slice(archievd_envelope.signature.as_slice());
                }

                let mut agent_public_key = [0u8; 32];
                if archievd_envelope.sender_public_key.len() == 32 {
                    agent_public_key
                        .copy_from_slice(archievd_envelope.sender_public_key.as_slice());
                }

                // UNIFIED PERIMETER AUDIT: Validates lineage token, proved the signature authenticity and checks path

                let (agent_hex, group_name) = match aegis.verify_session_lineage(
                    &archievd_envelope.sender_capability_cert.as_slice(),
                    &agent_public_key,
                ) {
                    Ok((agent, group)) => (agent, group),
                    Err(e) => {
                        eprintln!(
                            "[AEGIS NETWORK INTERDICTION] Dropped Malicious A2A RPC query line. Lineage Failed: {}",
                            e
                        );

                        continue;
                    }
                };

                let packet_timestamp: i64 = archievd_envelope.timestamp.into();

                if let Err(e) = aegis.authorize_packet_fast(
                    &agent_hex,
                    &group_name,
                    &agent_public_key,
                    question_payload,
                    &packet_signature,
                    &archievd_envelope.target_capability.as_str(),
                    packet_timestamp,
                ) {
                    eprintln!(
                        "[AEGIS NETWORK INTERDICTION] Dropped Malicious A2A RPC query line. Reason: {}",
                        e
                    );

                    continue;
                };

                // Execution approved. Invoke Local agent to Produce answer
                let answer_bytes = response_handler(question_payload);

                // Deliver reply back to the requester over zenoh
                if let Err(e) = query.reply(query.key_expr(), answer_bytes).await {
                    eprintln!("[A2A WARN] Failed to deliver query reply: {}", e);
                }
            }
        });
    }

    /// Dispatches a highly privileged system command directly to an agent's Python SDK
    pub async fn dispatch_control_override(&self, target_agent_hex: &str, system_prompt: &str) {
        let control_topic = format!("{}control/{}", self.workspace_prefix, target_agent_hex);

        // We use json here because the python sdk cintrol listeners needs to parse it easily
        let payload = serde_json::json!({
            "command": "FORCE_CONTEXT_EVICTION",
            "new_system_prompt": system_prompt,
            "timestamp": SystemTime::now().duration_since(UNIX_EPOCH).unwrap().as_secs()
        });

        // Fire the command across the Zenoh mesh
        if let Err(e) = self.session.put(&control_topic, payload.to_string()).await {
            eprintln!(
                "[ZENOH FATAL] Failed to dispath control overide to: {}: {}",
                target_agent_hex.to_string(),
                e
            )
        }
    }

    /// Broadcast session termination to the peer
    pub async fn shutdown(&self) {
        println!("[ZENOH] Broadcasting session termination to the global mesh...");
        // Close the session. This sends a 'Decl' (Declaration) to peer routers.
        let _ = self.session.close().await;
        println!("[ZENOH] Swarm servered cleanly. ");
    }
}
