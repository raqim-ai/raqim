use ed25519_dalek::SigningKey;
use notify::{Event, Watcher};
use rand_core::OsRng;
use raqim_core::aegis::{AegisConfigFile, AegisGateKeeper};
use raqim_core::api::{ApiState, UiEvent, build_admin_router};
use raqim_core::axon::AxonGateKeeper;

use axum::http::Method;
use raqim_core::compactor::WalCompactor;
use raqim_core::config::RaqimConfig;
use raqim_core::embedding::{EmbeddingProvider, LocalBgeProvider, OpenAIProvider};
use raqim_core::health::{HealthMonitor, SystemHealth};
use raqim_core::hot_memory::{HotVectorBuffer, HotVectorEntry};
use raqim_core::lancedb_store::LanceEngine;
use raqim_core::memory_router::MemoryRouter;
use raqim_core::network::GlobalNetworkBridge;
use raqim_core::nucleus::{WalCommand, WalEngine};
use raqim_core::registry::SwarmRegistry;
use raqim_core::state::SwarmStateRegistry;
use raqim_core::witness::WormWitnessEngine;
use raqim_core::{
    AgentState, IngressEnvelope, OpLog, RuntimeSecurityFlags, SystemEvent, execute_raqim_cascade,
};
use tower_http::cors::{Any, CorsLayer};

use std::path::Path;
use std::sync::Arc;
use std::{eprintln, fs, println};

use tokio::net::TcpListener;
use tokio::signal::unix::{SignalKind, signal};
use tokio::sync::broadcast;
use tokio::task::JoinSet;
use tokio_util::sync::CancellationToken;
use uuid::Uuid;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    println!("==================================================================");
    println!("Bismillah. Booting Hardened Raqim Sovereign Core Daemon v.1.0.0");
    println!("==================================================================");

    // =================================
    // SIGNAL INTERCEPTION (Graceful OS shutdown Handlers)
    // ================================
    let cancel_token = CancellationToken::new();
    let ct_clone = cancel_token.clone();

    tokio::spawn(async move {
        let mut sigterm = signal(SignalKind::terminate()).expect("Failed to bind SIGTERM");
        let mut sigint = signal(SignalKind::interrupt()).expect("Failed to bind SIGINT");

        tokio::select! {
            _ = sigterm.recv() => println!("\n[OS] Received SIGTERM from Kubernetes"),
            _ = sigint.recv() => println!("\n[OS] Received SIGINT (Ctrl+C) ")
        }

        println!("[SYSTEM] Initiating Sovereign Shutdown Sequence...");
        ct_clone.cancel();
    });

    let config = Arc::new(RaqimConfig::load_or_bootstrap());
    println!(
        "[CONFIG] Active Port: {} | WAL Path: {} ",
        &config.port, &config.wal_path,
    );

    // THE INTERNAL EVENT BUS
    let (event_tx, _event_rx) = broadcast::channel::<SystemEvent>(50_000);
    let (ui_tx, _ui_rx) = broadcast::channel::<UiEvent>(10_000);

    let registry = Arc::new(SwarmRegistry::new());
    let (health_tx, _health_rx) = broadcast::channel::<SystemHealth>(100);
    let (phantom_ui_tx, _phanom_ui_rx) = broadcast::channel::<UiEvent>(100);

    let security_flags = RuntimeSecurityFlags::new();

    // Securely loads the swarm key from disk. Generate it if it doesn't exist/
    let key_dir = Path::new("./ca-keys");
    let key_path = key_dir.join("swarm_master.key");

    // Generation Phase (First Boot Only)
    if !key_path.exists() {
        println!("[SECURITY] Initializing Swarm Master Cryptographic Root... ");
        fs::create_dir_all(key_dir).expect("Failed to create keys directory");

        let mut csprng = OsRng;
        let signing_key = SigningKey::generate(&mut csprng);

        fs::write(&key_path, signing_key.to_bytes()).expect("Failed to write Master Key");

        // Lock down Unix permissions
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            fs::set_permissions(&key_path, fs::Permissions::from_mode(0o600))
                .expect("Failed to secure Master Key Permissions");
        }
    }

    // Memory Load & Security Audit Phase
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let metadata = fs::metadata(&key_path).expect("FATAL: Failed to read master_key metadata");
        let permissions = metadata.permissions();
        let mode = permissions.mode();

        // Check if group or otthers have read/write/execute permission
        if mode & 0o77 != 0 {
            eprintln!(
                "[SECURITY HAZARD] Master key file '{:?}' has insecure permissions: {:o} (Expected 0600).",
                key_path,
                mode & 0o77
            );
            eprintln!("[SECURITY] Remedying file permission to 0600 (User read/write only)...");
            fs::set_permissions(&key_path, fs::Permissions::from_mode(0o600))
                .expect("FATAL: Failed to enforce 0600 permissions on master key");
        }
    }

    let key_bytes = fs::read(&key_path).expect("FATAL: Failed to read master_key from disk");
    let key_array: [u8; 32] = key_bytes
        .as_slice()
        .try_into()
        .expect("FATAL: Master key bytes is corruped (not 32 bytes)");
    let master_signing_key = SigningKey::from_bytes(&key_array);

    let master_public_key = master_signing_key.verifying_key().to_bytes();
    // ===============================
    let os_node_id = Uuid::new_v4().to_string();
    println!("[SYSTEM] Sovereign OS Node ID: {} ", os_node_id);

    // 1. BOOT SEQUENCE: INIITIALIZE ALL LAYERS (Wrapped in Arc for fearless concurrency)
    let brain_shard = Arc::new(SwarmStateRegistry::new());
    let axon = Arc::new(AxonGateKeeper::new());

    // LEAK-PROOF BROADCAST RECEIVER LOOP: Spawn consumer loop to detect lagging and evict slow readers automatically
    let mut system_events_subscriber = event_tx.subscribe();
    tokio::spawn(async move {
        loop {
            match system_events_subscriber.recv().await {
                Ok(event) => {
                    // Normal ingestion processing
                    if let SystemEvent::SystemBoot { message } = event {
                        println!("[BUS PROCESSING] Ingested boot message: {}", message);
                    }
                }

                // Memory Safeguard: Clear internal lags forcefully to protect RAM
                Err(broadcast::error::RecvError::Lagged(_)) => {}

                Err(broadcast::error::RecvError::Closed) => break,
            }
        }
    });

    // THE KERNEL AEGIS.TOML HOT-RELOAD WATCHER
    let policy_path_str = "aegis.toml";

    // Ensure policy manifest target exists beofre running file watcher
    if !Path::new(policy_path_str).exists() {
        let default_toml = r#"
        # Default Raqim Aegis Policy Manifest (Auto-Generated)
        [groups.admin_group]
        allowed_namespaces = ["*"]
        blocked_namespaces = []
        max_tps = 10000
        burst_capacity = 1000

        [groups.default_group]
        allowed_namespaces = ["/default/*"]
        blocked_namespaces = ["/admin/*", "/system/*"]
        max_tps = 100
        burst_capacity = 20
        "#;
        fs::write(policy_path_str, default_toml)?;
        println!("[AEGIS] Auto-generated default aegis.toml configuration file.");
    }

    let initial_content = fs::read_to_string(policy_path_str)?;
    let parsed_cfg = toml::from_str::<AegisConfigFile>(&initial_content)
        .expect("FATAL: Failed to parse initial aegis.toml configuration file");

    let initial_policies = parsed_cfg.to_group_policies();

    let aegis = Arc::new(AegisGateKeeper::new(
        initial_policies,
        &master_public_key,
        event_tx.clone(),
        ui_tx.clone(),
    ));

    let aegis_clone = aegis.clone();
    let (watch_tx, mut watch_rx) = tokio::sync::mpsc::channel::<Result<Event, notify::Error>>(10);

    // Spawn synchronous notify loop hooked directly to OS virtual filesystem events
    let mut watcher = notify::recommended_watcher(move |res| {
        let _ = watch_tx.blocking_send(res);
    })?;
    watcher.watch(
        Path::new(policy_path_str),
        notify::RecursiveMode::NonRecursive,
    )?;

    //  Async task consuming events transmitted out of the notify bridge loop
    tokio::spawn(async move {
        println!(
            "[KERNEL WATCHER] Listening for direct kernel modification on: {}... ",
            policy_path_str
        );

        while let Some(Ok(event)) = watch_rx.recv().await {
            // Check if modification contains a data close op (File update commit complete)
            if event.kind.is_modify() {
                println!("[KERNEL WATCHER] Change detected on policy file. Re-parsing metrics... ");

                // Read and re-parse file delta safely
                if let Ok(content) = fs::read_to_string("aegis.toml") {
                    match toml::from_str::<AegisConfigFile>(&content) {
                        Ok(parsed_cfg) => {
                            let fresh_policies = parsed_cfg.to_group_policies();
                            aegis_clone.reload_policies(fresh_policies);
                        }
                        Err(e) => {
                            // FAIL-SAFE
                            eprintln!(
                                "[AEGIS CONFIG ERROR] Invalid aegis.toml syntax: {}. Retaining previous security policies in RAM.",
                                e
                            );
                        }
                    }
                }
            }
        }
    });

    // Emit Initial system boot signal
    let _ = event_tx.send(SystemEvent::SystemBoot {
        message: "Raqim Sovereign Core Active.".to_string(),
    });

    let (wal, handle) = WalEngine::start(config.wal_path.clone()).await;
    let global_net = Arc::new(
        GlobalNetworkBridge::new(
            &security_flags.tenant_id.clone().read().unwrap(),
            &config.topic,
            aegis.clone(),
            os_node_id,
        )
        .await,
    );
    let hot_buffer = Arc::new(HotVectorBuffer::new(10_0000));

    let embedder: Arc<dyn EmbeddingProvider> = match config.embedder_type.as_str() {
        "openai" => {
            let key =
                std::env::var("OPENAI_API_KEY").unwrap_or_else(|_| config.openai_api_key.clone());
            if key.is_empty() {
                panic!("OPENAI_API_KEY environment variable or config entry is required");
            }
            Arc::new(OpenAIProvider::new(key))
        }

        _ => Arc::new(LocalBgeProvider::new()),
    };

    let lance_engine =
        Arc::new(LanceEngine::new(&config.lance_path, &config.table_name, embedder.clone()).await);

    // 1. Boot global Quarantine network subscriber
    global_net.listen_for_global_quarantine(aegis.clone()).await;

    let mem_router = Arc::new(MemoryRouter::new(
        config.clone(),
        axon.clone(),
        brain_shard.clone(),
        lance_engine.clone(),
        wal.clone(),
        event_tx.clone(),
    ));

    // 2. Wire SystemEvent subscriber loop for outbound local quarantine events
    let mut system_rx = event_tx.subscribe();
    let net_clone = global_net.clone();

    tokio::spawn(async move {
        while let Ok(event) = system_rx.recv().await {
            if let SystemEvent::GlobalQuarantineSync { record } = event {
                net_clone.broadcast_quarantine_sync(record).await;
            }
        }
    });

    // ============================
    // THE PHOENIX HYDRATION PROTOCOL: Reconstructs in-memory Axon Merkle trees from uncompacted WAL frames on boot.
    // ============================

    println!(
        "[INITIALIIZATION] Phoenix protocol: Commencing state rehydration scanning from active WAL frame sequences..."
    );
    let manifest_path = "compaction.manifest.json";
    let mut files_to_scan = Vec::new();

    // 2PC MANIFEST CHECK
    if Path::new(&manifest_path).exists() {
        if let Ok(content) = fs::read_to_string(manifest_path) {
            if let Ok(json_manifest) = serde_json::from_str::<serde_json::Value>(&content) {
                let state = json_manifest["state"].as_str().unwrap_or("");
                let target_file = json_manifest["target_file"].as_str().unwrap_or("");

                if state == "Committed" {
                    // Prevent Schizophrenia (Duplication): Data is in LanceDB, but WAL wasn't deleted.
                    println!(
                        "[PHONIX] Ghost WAL '{}' detected in COMMITTED state. Erasing to prevent Duplicate RAG Ingestion ",
                        &target_file
                    );
                    let _ = fs::remove_file(target_file);
                    let _ = fs::remove_file(manifest_path);
                } else if state == "PENDING" {
                    // Prevent Amnesia (Data Loss): File was rotated but never made it to lancedb
                    println!(
                        "[PHOENIX] Orphaned WAL '{}' detected in PENDING state. Queuing for RAM Hydration",
                        &target_file
                    );
                    if Path::new(&target_file).exists() {
                        files_to_scan.push(target_file.to_string());
                    }
                }
            }
        }
    }

    // Scans the active WAL last so temporal order is preserved
    if Path::new(&config.wal_path).exists() {
        files_to_scan.push(config.wal_path.clone())
    }

    let witness_engine = Arc::new(WormWitnessEngine::new(
        &config.witness_path,
        master_signing_key.clone(),
        None,
    ));

    let mut recovered_logs: Vec<OpLog> = Vec::new();
    let mut uncompacted_count = 0;

    // Execute assembled scanning over the assembled timeline array
    for file_path in files_to_scan {
        println!("[PHOENIX] Scanning {} ...", file_path);

        if let Ok(wal_bytes) = fs::read(&file_path) {
            let mut offset = 0;
            let mut aligned_buf = rkyv::util::AlignedVec::<16>::new();

            while offset + 8 <= wal_bytes.len() {
                let entry_len =
                    u32::from_le_bytes(wal_bytes[offset..offset + 4].try_into().unwrap()) as usize;
                let expected_crc =
                    u32::from_le_bytes(wal_bytes[offset + 4..offset + 8].try_into().unwrap());
                let frame_total = 8 + entry_len;

                if offset + frame_total > wal_bytes.len() {
                    eprintln!(
                        "[PHOENIX WARN] Truncateed tail frame in {}. Halting scan",
                        file_path
                    );
                    break;
                }

                let entry_slice = &wal_bytes[offset + 8..offset + frame_total];

                if crc32fast::hash(entry_slice) != expected_crc {
                    eprintln!(
                        "[PHOENIX CORRUPTION] CRC32 mismatch in {}. Trncating tail.",
                        file_path
                    );
                    break;
                }

                // Force 16-byte alignment for u128 uuidv7 zero-copy validation
                aligned_buf.clear();
                aligned_buf.extend_from_slice(entry_slice);

                if let Ok(archived_log) = rkyv::access::<
                    <Vec<OpLog> as rkyv::Archive>::Archived,
                    rkyv::rancor::Error,
                >(&aligned_buf)
                {
                    if let Ok(batch) =
                        rkyv::deserialize::<Vec<OpLog>, rkyv::rancor::Error>(archived_log)
                    {
                        for recovered_log in batch {
                            axon.hydrate_from_recovery(&recovered_log);

                            // Fetch crdt shard for this namespace and apply historical delta
                            let brain =
                                brain_shard.get_or_create_brain(&recovered_log.state.namespace);
                            if let Err(e) = brain.assimilate_foreign_thought(&recovered_log.delta) {
                                eprintln!(
                                    "[PHOENIX WARN] Failed to assimilate CRDT delta during recovery: {},",
                                    e
                                );
                            }

                            recovered_logs.push(recovered_log);
                            uncompacted_count += 1
                        }
                    }
                }

                offset += frame_total;
            }
        }
    }

    println!(
        "[PHOENIX] Rehydrated {} un-crystallized log frames into Axon buffer ",
        uncompacted_count
    );

    if !recovered_logs.is_empty() {
        // Trailing 250 thoughts
        let cache_limit = 250.min(recovered_logs.len());
        let recent_logs_slice = &recovered_logs[recovered_logs.len() - cache_limit..];

        println!(
            " [PHOENIX] Batch-embedding trailing {} thoughts to warm uo HotVectorBuffer...",
            recent_logs_slice.len()
        );

        // Batch embed all recovered WAL texts to restore hot vector memory
        let texts: Vec<String> = recent_logs_slice
            .iter()
            .map(|l| {
                format!(
                    "[{:?}] Agent in {} stated {}",
                    l.state.status, l.state.namespace, l.state.text
                )
            })
            .collect();

        // Recompute the vector space asynchronouly to bypass short-term amnesia gaps
        // Simulated blocks, maps out directly to fastembed / OpenAI endpoints
        // let mock_embed_vectors = vec![vec![0.0f32; 768]; recent_logs_slice.len()];

        if let Ok(vectors) = embedder.embed_batch(&texts).await {
            let mut hot_entries = Vec::with_capacity(recent_logs_slice.len());
            for (i, log) in recent_logs_slice.into_iter().enumerate() {
                hot_entries.push(HotVectorEntry {
                    tx_id: log.state.transaction_id,
                    agent_hex: hex::encode(log.agent_id),
                    namespace: log.state.namespace.clone(),
                    text: log.state.text.clone(),
                    timestamp: log.state.timestamp,
                    vector: vectors[i].clone(),
                });
            }
            let entry_count = hot_entries.len();
            hot_buffer.push_batch(hot_entries);
            println!(
                "[INITIALIZATION] Phoenix Boot Hydration complete. Restored {} hot vectors in RAM.",
                entry_count
            );
        }
    }

    // Load un-tamperable chronological WORM roots and assert execution matrix match
    let anchored_witness = witness_engine.load_local_witness();
    if !anchored_witness.is_empty() {
        axon.execute_forensic_boot_audit(&anchored_witness, &witness_engine.clone())
            .await?;
    }

    // ---  COMPACTION EVENT LISTENENR  (Watermark eviction) ----
    let mut system_rx = event_tx.subscribe();
    let hot_buffer_clone = hot_buffer.clone();

    tokio::spawn(async move {
        while let Ok(event) = system_rx.recv().await {
            if let SystemEvent::CompactionTriggered {
                max_compacted_tx, ..
            } = event
            {
                hot_buffer_clone.evict_compacted_up_to(max_compacted_tx);
            }
        }
    });

    // We spawn the Audit Vault Sinker. This OS thread's ONLY job is to listen to the internal event bus
    let mut valut_rx = event_tx.subscribe();
    let lance_vault_clone = lance_engine.clone();
    let lance_net = global_net.clone();

    tokio::spawn(async move {
        println!("[SYSTEM] Audit Valult Telemetry Sinker Active.");

        while let Ok(event) = valut_rx.recv().await {
            lance_vault_clone.log_system_events(&event).await;

            match event {
                SystemEvent::GlobalQuarantineSync { record } => {
                    lance_net.broadcast_quarantine_sync(record).await;
                }

                _ => {}
            }
        }
    });

    // The Autonomous compactor (WAL reaper)
    let compactor = Arc::new(WalCompactor::new(
        &config.wal_path,
        &config.manifest_path,
        lance_engine.clone(),
        event_tx.clone(),
        wal.cmd_sender.clone(),
    ));

    // Start Autonomous Daemon
    compactor.clone().start_daemon();

    // 2 Background Listeners (Zenoh Global network)
    let global_net_clone = global_net.clone();
    let global_axon = axon.clone();
    let global_brain = brain_shard.clone();
    let global_tx = event_tx.clone();
    tokio::spawn(async move {
        global_net_clone
            .listen_for_foreign_thoughts(global_brain, global_axon, global_tx)
            .await;
    });

    let (pause_tx, pause_rx) = tokio::sync::watch::channel(false);
    let pause_tx = Arc::new(pause_tx);

    // Spawn the hardware interrupt loop
    let health_pause_rx = pause_rx.clone();
    HealthMonitor::spawn_telemetry_loop(health_tx.clone(), health_pause_rx);

    let api_state = ApiState {
        config: config.clone(),
        aegis: aegis.clone(),
        mem_router: mem_router.clone(),
        global_net: global_net.clone(),
        axon: axon.clone(),
        brain: brain_shard.clone(),
        lance: lance_engine.clone(),
        wal: wal.clone(),
        event_tx: event_tx.clone(),

        ui_tx: ui_tx.clone(),
        phantom_ui_tx: phantom_ui_tx.clone(),
        health_tx: health_tx.clone(),
        swarm_registry: registry.clone(),
        master_signing_key: master_signing_key.clone(),

        hot_buffer: hot_buffer.clone(),
        pause_tx: pause_tx.clone(),
        compactor: compactor.clone(),
    };

    let axum_app = build_admin_router(api_state).layer(
        CorsLayer::new()
            .allow_origin(Any)
            .allow_methods([Method::GET, Method::POST])
            .allow_headers(Any),
    );
    let api_port = config.port + 1;
    tokio::spawn(async move {
        let listener = TcpListener::bind(format!("0.0.0.0:{}", api_port))
            .await
            .unwrap();
        println!("[SYSTEM] Axum control plane live on port {} ", api_port);
        axum::serve(listener, axum_app).await.unwrap();
    });

    // 3. The Production TCP ingress.
    let listener = TcpListener::bind(format!("0.0.0.0:{}", config.port))
        .await
        .unwrap();
    println!("Organism live. Awaiting LLM Agent TCP Connections...");

    // JoinSet automatically tracks all spawned TCP worker tasks.
    let mut tcp_workers = JoinSet::new();

    // Limit max concurrent live TCP agent sockets.
    let connection_semaphore = Arc::new(tokio::sync::Semaphore::new(512));

    loop {
        tokio::select! {
                    // If cancelled is triggered, break the infinite loop.
                    _ = cancel_token.cancelled() => {
                        println!("[NETWORK] TCP Ingress halted. Rejecting new connections. ");
                        break;
                    }

                // Otherwise, Accept connections normally.
                accpet_res = listener.accept() => {
                    let (socket, addr) = match accpet_res {
                        Ok(res) => res,
                        Err(_) => continue
                    };


                    let permit = match connection_semaphore.clone().try_acquire_owned() {

                        Ok(p) => p,
                        Err(_) => {
                            eprintln!("[INGRESS THROTTLED] Connectiton limit (512) reached. Dropping socket from {}", addr);
                            drop(socket);
                            continue;
                        }

                    };


                    println!("External Agent connected from: {}", addr);

                    let task_axon = axon.clone();
                    let task_wal = wal.clone();
                    let global_publisher = global_net.clone();
                    let task_event_tx = event_tx.clone();
                    let task_aegis = aegis.clone();
                    let task_ui_tx = ui_tx.clone();
                    let task_registry = registry.clone();
                    let task_brain = brain_shard.clone();
                    let task_mem_router = mem_router.clone();
                    let task_pause_rx = pause_rx.clone();


                // Spawn into the joinset
            tcp_workers.spawn(async move {
                let _permit = permit;

                // split socket into independent read and write halves
                let (read_half, mut write_half) = socket.into_split();

                //  Syscall Amortization: Wrap the socket in a 1mb BufReader to eliminate kernel context switches
                let mut reader = tokio::io::BufReader::with_capacity(1024 * 1024, read_half);

                // Heap Allocation Amortization: pre-allocate a 1mb scratch buffer ONCE to eliminate dynamic heap allocation.
                let mut payload_scratch_buf = vec![0u8; 1024* 1024];

                // ENTERPRISE FIX: Socket-Level Cryptographic Session Cache.
                let mut session_established = false;
                let mut cached_agent_hex = String::new();
                let mut cached_group_name = String::new();
                let mut session_pub_key = [0u8; 32];
                let mut worker_pause_rx = task_pause_rx.clone();

                loop {
                    // ZERO-CPU ASYNC SUSPENSION:
                    if *worker_pause_rx.borrow() {
                        if worker_pause_rx.changed().await.is_err() {
                            break;
                        }

                        if *worker_pause_rx.borrow() {
                            continue;
                        }

                    }

                   //  THE FRAMING PROTOCOL: Read 4-byte length prefix first
                    let mut len_buf = [0u8; 4];
                    if let Err(e) = tokio::io::AsyncReadExt::read_exact(&mut reader, &mut len_buf).await {

                            if e.kind() == std::io::ErrorKind::UnexpectedEof {
                                println!("[TCP EDGE] Agent at {} disconnected cleanly (EOF) ", addr);
                            } else {


                                eprintln!("[TCP EDGE]: Connection closed or read failed: {}", e);
                            }
                        break;
                    }
                    let payload_len = u32::from_le_bytes(len_buf) as usize;

                    // Prevent malicious massive memory allocation attacks ( Max 1mb per thought )
                    if payload_len > 1024 * 1024 {
                        eprintln!("[NETWORK WARN] Payload Exceeded 1MB limit. Dropping Connections.");
                        break;
                    }

                    // Read the exact payload bytes: Read into the preallocated screatch bufffer.
                    // We slice the scratch buffer to the exact length of the incoming payload complately bypassing the OS memory allocator
                    let active_payload_slice : &mut [u8] = &mut payload_scratch_buf[0..payload_len];

                    if let Err(e) = tokio::io::AsyncReadExt::read_exact(&mut reader, active_payload_slice).await {
                        eprintln!("[TCP EDGE]: Failed to load TCP payload {}", e);
                        break;
                    }

                let archived_ingress = match rkyv::access::<<IngressEnvelope as rkyv::Archive>::Archived, rkyv::rancor::Error>(active_payload_slice) {
                    Ok(valid_archived) => valid_archived,
                    Err(e) => {
                        eprintln!("[AEGIS] TCP Dropped: Malformed Memory layout (IngressEnvelope): {}", e);
                        break;
                    }
                };

                let path_intent = archived_ingress.intent_path.as_str();
                let state_slice = archived_ingress.state_bytes.as_slice();


                // ===== REALIGNMENT: Force the sub-slice onto machine word boundaries ==========
                    let mut aligned_state_buf: rkyv::util::AlignedVec<16> = rkyv::util::AlignedVec::new();
                    aligned_state_buf.extend_from_slice(state_slice);

                    // Validates the memory layout over the aligned buffer allocation
                    let archived_state = match rkyv::access::<<AgentState as rkyv::Archive>::Archived, rkyv::rancor::Error>(&aligned_state_buf) {
                        Ok(valid_state) => valid_state,
                        Err(e) => {
                            eprintln!("[AEGIS ERROR] TCP Dropped: Misaligned/Malformed Inner Payload (AgentState): {} ", e);
                            break;
                        }
                    };

                    let agent_pub_key: [u8; 32] = archived_ingress.public_key.try_into().unwrap_or([0; 32]);
                    let mut packet_sig = [0u8; 64];
                    packet_sig.copy_from_slice( archived_ingress.signature.as_slice() );

                    // UNIFIED PERIMETER: Validates lineage, check signature. ONLY verify the heavy Master Certificate on the very first packet.
                    if !session_established {

                            match task_aegis.verify_session_lineage(archived_ingress.capability_cert.as_slice(), &agent_pub_key) {
                                Ok((agent_hex, group_name)) => {
                                    session_established = true;
                                    cached_agent_hex = agent_hex;
                                    cached_group_name = group_name;
                                    session_pub_key = agent_pub_key;
                                }

                                Err(e) => {
                                    eprintln!("[AEGIS INTERDICTION] Handshake Failed: {} ", e);
                                    break;
                                }
                            }
                    }   else {
                        // Enforce Key Consistency: Detect public key swiitching on active session
                        if agent_pub_key != session_pub_key {
                            eprintln!("[AEGIS INTERDICTION] Key Drift Attack detected. Dropping Socket. ");
                            break;
                        }
                    }

                    // Perform ultrafast packet audit for each packet.
                    let packet_timestamp = archived_state.timestamp.into();
                    if let  Err(e) = task_aegis.authorize_packet_fast(&cached_agent_hex, &cached_group_name, &agent_pub_key, state_slice, &packet_sig, path_intent, packet_timestamp) {
                        eprintln!("[AEGIS INTERDICTION] Fast Audit failed: {} ", e);
                        break;
                    }

                    let agent_hex = cached_agent_hex.clone();

                    let text = archived_state.text.as_str().to_string();

                    let mut alias = "Unknown".to_string();
                    if path_intent == "/system/handshake" {
                        if text.starts_with("ALIAS=") {
                            let alias = text.replace("ALIAS=", "").trim().to_string();
                            // We do not execute a cascade for handshake. We just register and drop
                            task_registry.touch_agent(&agent_hex, &path_intent, "Connected", &alias);

                            // ==============================
                            // JIT COLD-START HYDRATION
                            // ==============================
                            let agent_hex_clone = agent_hex.clone();
                            let wal_clone = task_wal.clone();
                            let router_clone = task_mem_router.clone();

                            // spawn the heave lancedb/wal lookup in the bg os thread
                            tokio::spawn(async move {

                                println!("[JIT HYDRATION] Waking up agent {} from cold storage.", agent_hex_clone);

                                if let Err(e) = router_clone.rebuild_agent_timeline(&agent_hex_clone, u128::MAX, wal_clone).await {

                                        eprintln!(" [JIT HYDRATION ERROR] Failed to wake up agent {}: {} ", agent_hex_clone, e);

                                } else {
                                    println!("[JIT HYDRATION] Agent {} fully synchronized with historical reality.", agent_hex_clone);
                                }
                            });

                            continue;
                        }
                    } else {
                        // O(1) Ram lookup and keep the alias active for normal thought
                        if let Some(agent_proc) = task_registry.active_agents.get(&agent_hex) {
                            alias = agent_proc.alias.clone();
                        }
                    }

                    // --- The Raqim Cascade ---
                    // If the WAL or the Publisher channel are full, the .await creates a healthy backppressure rather than panicking.
                    match execute_raqim_cascade(
                        &archived_state,
                        task_axon.clone(),
                        task_wal.clone(),
                        task_brain.clone(),
                        global_publisher.clone(),
                        task_event_tx.clone(),
                        Vec::new(),
                        Vec::new(),
                    )
                    .await {

                        Ok(tx_id) => {

                                // Emit 20 byte server ack frame [4 bytes: Status] + [16 bytes: Little-Endian u128 TxID]
                            let mut ack_buf = [0u8; 20];
                            ack_buf[0..4].copy_from_slice(&0u32.to_le_bytes());
                            ack_buf[4..20].copy_from_slice(&tx_id.to_le_bytes());

                            if let Err(e) = tokio::io::AsyncWriteExt::write_all(&mut write_half, &ack_buf).await {
                                eprintln!("[TCP EDGE] Failed to deliver ACK frame: {} ", e);
                                break;
                            }

                            let _ = task_ui_tx.send(UiEvent::ThoughtCommitted {
                                agent_hex: agent_hex.clone(),
                                intent_path: path_intent.to_string(),
                                tx_id: format!("{:032x}", tx_id),
                                text,
                            });
                     }
                     Err(e) => {
                        eprintln!("[TCP INGRESS REJECTED] Hardware Backpressure: {} ", e);

                        let mut err_buf = [0u8; 20];
                        err_buf[0..4].copy_from_slice(&1u32.to_le_bytes());

                        let _ = tokio::io::AsyncWriteExt::write_all(&mut write_half, &err_buf).await;
                        break;


                    }


                    }

                    // Update RAM process Table (O(1) nanoseconds lock)
                    task_registry.touch_agent(
                        agent_hex.clone().as_str(),
                        archived_ingress.intent_path.as_str(),
                        "Active",
                        &alias,
                    );

                 }

                });



            }
        }
    }

    // =========================
    // GRACEFUL DRAIN
    // =========================

    // Drain In-Flight TCP packets
    println!(
        "[SYSTEM] Draining {} active TCP threads... ",
        tcp_workers.len()
    );
    while let Some(res) = tcp_workers.join_next().await {
        if let Err(e) = res {
            eprint!(
                "[SYSTEM WARN] A TCP worker panicked during shutdown: {} ",
                e
            );
        }
    }

    println!("[SYSTEM] All active thoughts processed and sealed.");

    // Sever the Global Mesh
    global_net.shutdown().await;
    let _ = wal.cmd_sender.send(WalCommand::Shutdown).await;
    // Seal the WAL safely to nvme
    drop(wal);
    println!("[WAL] Senders dropped. Awaiting final io_uring fsync to NVMe... ");
    let _ = handle.await;

    println!("[SYSTEM] Raqim OS terminated cleanly. Zero data loss. AlhamdulliLah.");

    Ok(())
}
