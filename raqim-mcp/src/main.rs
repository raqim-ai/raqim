use ed25519_dalek::{Signer, SigningKey};
use futures_util::{SinkExt, StreamExt};
use mcp_rust_sdk::error::ErrorCode;
use mcp_rust_sdk::server::{Server, ServerHandler};
use mcp_rust_sdk::transport::stdio::StdioTransport;
use mcp_rust_sdk::types::{ClientCapabilities, Implementation, ServerCapabilities, Tool};
use raqim_core::api::WsMessage;
use serde_json::{Value, json};
use std::net::{Ipv4Addr, SocketAddr, SocketAddrV4};
use std::sync::Arc;
use std::{eprintln, format, fs, println};

use async_trait::async_trait;
use raqim_core::{AgentState, AgentStatus, IngressEnvelope};
use std::time::{Duration, SystemTime, UNIX_EPOCH};
use tokio::io::AsyncWriteExt;
use tokio::net::TcpStream;

// 1. Define our custom handler struct
struct RaqimHandler {
    signing_key: SigningKey,
    pub_key_bytes: [u8; 32],
    capability_cert_bytes: Vec<u8>,
    daemon_http_url: String,
    daemon_tcp_addr: SocketAddr,
    http_client: reqwest::Client,
    commit_tool: Tool,
    query_tool: Tool,
    ask_swarm_tool: Tool,
}

impl RaqimHandler {
    fn new(private_key_path: &str, cert_path: &str, tcp_port: u16, daemon_http_url: &str) -> Self {
        println!("[MCP HANDLER] Loading Cryptographic Identity and capability credentials...  ");

        // Load Signing Private Key
        let key_bytes = fs::read(private_key_path).unwrap_or_else(|_| {
            panic!(
                " FATAL: Missing Private Key at '{}'. Aegis identity key required. ",
                private_key_path
            )
        });

        let key_array: &[u8; 32] = key_bytes
            .as_slice()
            .try_into()
            .expect(" FATAL: Invalid Private Key length (32 bytes required) ");

        let signing_key = SigningKey::from_bytes(key_array);
        let pub_key_bytes = signing_key.verifying_key().to_bytes();

        // Load signed capability cert
        let capability_cert_bytes = fs::read(cert_path).unwrap_or_else(|_| {
            eprintln!("[MCP WARNING] Capability certificate not found at '{}'. Handshake will fail if aegis enforces lineage.", cert_path);

            Vec::new()

        } );

        let http_client = reqwest::Client::builder()
            .timeout(Duration::from_secs(10))
            .build()
            .expect("Failed to initialize HTTP client");

        let daemon_tcp_addr =
            SocketAddr::V4(SocketAddrV4::new(Ipv4Addr::new(127, 0, 0, 1), tcp_port));

        Self {
            signing_key,
            pub_key_bytes,
            capability_cert_bytes,
            http_client,
            daemon_http_url: daemon_http_url.to_string(),
            daemon_tcp_addr,
            ask_swarm_tool: Tool {
                name: "ask_swarm".to_string(),
                description: "Ask another agent a question via the A2A Zero-Trust network"
                    .to_string(),
                schema: json!({
                    "type": "object",
                    "properties": {
                        "target_capability": {"type": "string", "description": "e.g. /finance/fraud_eval"},
                        "question": {"type": "string", "description": "The precise investigative query"}
                    },
                    "required": ["target_capability", "question"]
                }),
            },

            commit_tool: Tool {
                name: "commit_thought".to_string(),
                description: "Commits a cryptographically verified decision to the Raqim OS CRDT and  Merkle DAG ".to_string(),
                schema: json!({
                    "type": "object",
                    "properties": {
                        "thought_text": {"type": "string", "description": "The reasoning or action payload"},
                        "status": {"type": "string", "enum": ["Reasoning", "ToolExecution", "Halted", "Idle"]},
                        "intent_path": {"type": "string", "description": "Target namespace e.g /finance/audit"},
                    },
                    "required": ["thought_text", "status", "intent_path", "agent_id_hex"]
                }),
            },

            query_tool: Tool {
                name: "query_memory".to_string(),
                description: "Semantic Hyybrid RAG search across Hot RAM and Cold LanceDB storage.".to_string(),
                schema: json!({
                    "type": "object",
                    "properties": {
                        "query": {"type": "string", "description": "Natural language query to search for"},
                        "intent_path": {"type": "string", "description": "Target namespace filter (e.g. /finance/*)" }

                    },
                    "required": ["query", "intent_path"]
                }),
            },
        }
    }
}

#[async_trait]
impl ServerHandler for RaqimHandler {
    async fn initialize(
        &self,
        _client_info: Implementation,
        _client_caps: ClientCapabilities,
    ) -> Result<ServerCapabilities, mcp_rust_sdk::Error> {
        // let mut tool_cap = HashMap::new();
        // tool_cap.insert("listChanged".to_string(), json!(false));

        Ok(ServerCapabilities { custom: None })
    }

    // 2.The Router, handles Tool Discovery
    async fn handle_method(
        &self,
        method: &str,
        params: Option<Value>,
    ) -> Result<Value, mcp_rust_sdk::Error> {
        match method {
            // LLM asks: "What tools do you have?"
            "tools/list" => Ok(
                json!({"tools": [self.commit_tool.clone(), self.query_tool.clone(), self.ask_swarm_tool.clone()]}),
            ),

            // LLM says: "Execute this tool!"
            "tools/call" => {
                let p = params.ok_or_else(|| {
                    mcp_rust_sdk::Error::protocol(ErrorCode::InvalidParams, "Missing params")
                })?;
                let name = p.get("name").and_then(|v| v.as_str()).unwrap_or("");
                let args = p.get("arguments").ok_or_else(|| {
                    mcp_rust_sdk::Error::protocol(ErrorCode::InvalidParams, "Missing args")
                })?;

                if name == "commit_thought" {
                    // Mathematical derivation. (The absolute Truth)
                    // The hash the exact public key that was used to initialize this MCP Server instance.
                    let mut hasher = blake3::Hasher::new_derive_key("raqim.agent.v1.identity");
                    hasher.update(&self.pub_key_bytes);
                    let mut derived_16_bytes = [0u8; 16];
                    hasher.finalize_xof().fill(&mut derived_16_bytes);

                    // --- Translation layer ----
                    let intent_path = args
                        .get("intent_path")
                        .unwrap()
                        .as_str()
                        .unwrap()
                        .to_string();

                    let text = args
                        .get("thought_text")
                        .unwrap()
                        .as_str()
                        .unwrap()
                        .to_string();

                    let status_str = args.get("status").unwrap().as_str().unwrap();
                    let status = match status_str {
                        "Reasoning" => AgentStatus::Reasoning,
                        "ToolExecution" => AgentStatus::ToolExecution,
                        "Halted" => AgentStatus::Halted,
                        _ => AgentStatus::Idle,
                    };

                    // Translate into Raqim core logic
                    let state = AgentState {
                        agent_id: Some(derived_16_bytes.try_into().unwrap_or([0; 16])),
                        transaction_id: uuid::Uuid::now_v7().as_u128(),
                        namespace: intent_path.clone(),
                        timestamp: SystemTime::now()
                            .duration_since(UNIX_EPOCH)
                            .unwrap()
                            .as_secs() as i64,
                        status,
                        text: text.clone(),
                    };

                    // THE CRYPTOGRAPHIC ENVELOPE
                    //  Hash the state bytes.
                    let state_bytes = rkyv::to_bytes::<rkyv::rancor::Error>(&state)
                        .map_err(|e| {
                            mcp_rust_sdk::Error::Other(format!("State serialization failed: {}", e))
                        })?
                        .into_vec();

                    // Mathematically sign the state bytes with our private key.
                    let signature = self.signing_key.sign(&state_bytes).to_bytes();

                    let envelope = IngressEnvelope {
                        intent_path,
                        public_key: self.pub_key_bytes,
                        signature,
                        state_bytes,
                        capability_cert: self.capability_cert_bytes.clone(),
                    };

                    // Zero-copy serialize the state
                    let serialized_envelope = rkyv::to_bytes::<rkyv::rancor::Error>(&envelope)
                        .map_err(|e| {
                            mcp_rust_sdk::Error::Other(format!(
                                "Envelope Serialization failed: {}",
                                e
                            ))
                        })?
                        .into_vec();
                    let payload_len = (serialized_envelope.len() as u32).to_le_bytes();

                    // Fire to the running Raqim daemon Over TCP
                    if let Ok(mut stream) = TcpStream::connect(self.daemon_tcp_addr).await {
                        stream
                            .write_all(&payload_len)
                            .await
                            .map_err(|e| mcp_rust_sdk::Error::Other(e.to_string()))?;
                        stream
                            .write_all(&serialized_envelope)
                            .await
                            .map_err(|e| mcp_rust_sdk::Error::Other(e.to_string()))?;
                        stream
                            .flush()
                            .await
                            .map_err(|e| mcp_rust_sdk::Error::Other(e.to_string()))?;

                        //  Tell the LLM it succeeded
                        return Ok(json!({
                           "content": [{
                               "type": "text",
                               "text": format!(" Thought commited to WAL: TxID 0x{:032x}", state.transaction_id)
                           }]
                        }));
                    }

                    Err(mcp_rust_sdk::Error::Other(
                        "Failed to connect to Raqim Deamon TCP".into(),
                    ))
                } else if name == "query_memory" {
                    let intent_path = args.get("intent_path").unwrap().as_str().unwrap();
                    let query = args.get("query").unwrap().as_str().unwrap();

                    let url = format!(
                        "{}/v1/swarm/memory?namespace={}&query={}",
                        self.daemon_http_url, intent_path, query
                    );

                    // AXUM HTTP RAG CALL
                    let req = self.http_client.get(&url);

                    let response = req
                        .send()
                        .await
                        .map_err(|e| mcp_rust_sdk::Error::Other(e.to_string()))?;

                    if response.status().is_success() {
                        let memories: Vec<String> =
                            response.json::<Vec<String>>().await.unwrap_or_default();
                        return Ok(
                            json!({"content": [{"type": "text", "text": if memories.is_empty() {"No relevant historical records found".to_string()} else {format!("Retreived Historical Context: \n{}", memories.join("\n"))}}]}),
                        );
                    } else {
                        return Err(mcp_rust_sdk::Error::Other(format!(
                            " RAG Query failed with status {}",
                            response.status()
                        )));
                    }
                } else if name == "ask_swarm" {
                    let target_capability = args
                        .get("target_capability")
                        .unwrap()
                        .as_str()
                        .unwrap()
                        .to_string();
                    let question_text = args.get("question").unwrap().as_str().unwrap().to_string();
                    let request_id = uuid::Uuid::now_v7().to_string();

                    // Compute true Blake3 sender_hex
                    let mut hasher = blake3::Hasher::new_derive_key("raqim.agent.v1.identity");
                    hasher.update(&self.pub_key_bytes);
                    let mut derived_16_bytes = [0u8; 16];
                    hasher.finalize_xof().fill(&mut derived_16_bytes);
                    let sender_hex = hex::encode(derived_16_bytes);

                    // 1. SIGN THE QUESTION
                    let question_bytes = question_text.into_bytes();
                    let signature = self.signing_key.sign(&question_bytes).to_bytes();

                    let ask_msg = WsMessage::AskQuestion {
                        request_id: request_id.clone(),
                        capability: target_capability,
                        question: question_bytes,
                        sender_hex,
                        public_key: hex::encode(self.pub_key_bytes),
                        signature: signature.to_vec(),
                        capability_cert: hex::encode(&self.capability_cert_bytes),
                    };

                    // 2. Connect to RQM Daemon Websocket.
                    let ws_url = format!(
                        "{}/v1/mcp/ws",
                        self.daemon_http_url
                            .replace("http://", "ws://")
                            .replace("https://", "wss://")
                    );
                    let (mut ws_stream, _) = tokio_tungstenite::connect_async(&ws_url)
                        .await
                        .map_err(|e| {
                            mcp_rust_sdk::Error::Other(format!("WS Connect Failed: {}", e))
                        })?;

                    // 3. SEND THE REQUEST
                    let json_payload = serde_json::to_string(&ask_msg).unwrap();
                    ws_stream
                        .send(tokio_tungstenite::tungstenite::Message::Text(
                            json_payload.into(),
                        ))
                        .await
                        .map_err(|e| mcp_rust_sdk::Error::Other(e.to_string()))?;

                    // 4. AWAIT THE RESPONSE (With Timeout)
                    let response = tokio::time::timeout(Duration::from_secs(15), async {
                        while let Some(Ok(msg)) = ws_stream.next().await {
                            if let tokio_tungstenite::tungstenite::Message::Text(text) = msg {
                                if let Ok(WsMessage::QuestionAnswered {
                                    request_id: incoming_id,
                                    answer,
                                }) = serde_json::from_str(&text)
                                {
                                    if incoming_id == request_id {
                                        return Ok(String::from_utf8_lossy(&answer).to_string());
                                    }
                                } else if let Ok(WsMessage::Error { message }) =
                                    serde_json::from_str::<WsMessage>(&text)
                                {
                                    return Err(message);
                                }
                            }
                        }

                        Err("WebSocket closed unexpectedly".to_string())
                    })
                    .await;

                    match response {
                        Ok(Ok(answer_text)) => {
                            let _ = ws_stream.close(None).await; // Graceful cleanup 
                            return Ok(json!({"content": [{"type": "text", "text": answer_text}]}));
                        }
                        Ok(Err(e)) => return Err(mcp_rust_sdk::Error::Other(e)),
                        Err(_) => {
                            return Err(mcp_rust_sdk::Error::Other(
                                "A2A Timeout exceeed 15s".to_string(),
                            ));
                        }
                    }
                } else {
                    return Err(mcp_rust_sdk::Error::protocol(
                        ErrorCode::MethodNotFound,
                        "Unknown tool",
                    ));
                }
            }

            _ => Err(mcp_rust_sdk::Error::protocol(
                ErrorCode::MethodNotFound,
                "Method not supported",
            )),
        }
    }

    // 4. Clean shutdowm
    async fn shutdown(&self) -> Result<(), mcp_rust_sdk::Error> {
        Ok(())
    }
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    println!("Bismillah. Booting Raqim MPC Universal Translator... ");

    // Load from environment
    let key_path =
        std::env::var("RQM_MCP_KEY_PATH").unwrap_or_else(|_| "./keys/mcp_private.pem".to_string());

    let cert_path =
        std::env::var("RQM_MCP_CERT_PATH").unwrap_or_else(|_| "./keys/mcp_cert.pem".to_string());

    let daemon_url =
        std::env::var("RQM_DEAMON_URL").unwrap_or_else(|_| "http://127.0.0.1:8081".to_string());

    let (transport, _message_sender) = StdioTransport::new();

    let handler = Arc::new(RaqimHandler::new(&key_path, &cert_path, 8080, &daemon_url));
    let server = Server::new(Arc::new(transport), handler as Arc<dyn ServerHandler>);

    server.start().await?;

    Ok(())
}
