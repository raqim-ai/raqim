use crate::{
    AgentStatus, OpLog,
    api::{TimelineNode, VaultSearchResult},
};
use aho_corasick::AhoCorasick;
use memmap2::MmapOptions;
use rkyv::to_bytes;

use std::{
    collections::BTreeMap,
    eprintln,
    fs::File,
    io::Read,
    println,
    sync::Arc,
    time::{Duration, SystemTime, UNIX_EPOCH},
    vec,
};
use tokio::io::AsyncWriteExt;
use tokio::{
    io::AsyncSeekExt,
    sync::{RwLock, mpsc, oneshot},
    time::interval,
};

pub struct WalEngine {
    sender: mpsc::Sender<OpLog>,
    pub cmd_sender: mpsc::Sender<WalCommand>,

    // The O(1) INDEX: Maps TxID -> Physical byte offset in the WAL.
    pub index: Arc<RwLock<BTreeMap<u128, u64>>>,
}

#[derive(Debug)]
pub enum WalError {
    IngressQueueFull(String),
    Io(std::io::Error),
}

impl std::fmt::Display for WalError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            WalError::IngressQueueFull(msg) => write!(f, "WAL channel saturated: {}", msg),
            WalError::Io(e) => write!(f, "I/O error during WAL operation: {}", e),
        }
    }
}

impl std::error::Error for WalError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            WalError::IngressQueueFull(_) => None,
            WalError::Io(e) => Some(e),
        }
    }
}

impl From<std::io::Error> for WalError {
    fn from(err: std::io::Error) -> Self {
        WalError::Io(err)
    }
}

pub enum WalCommand {
    Rotate(oneshot::Sender<String>),
    Shutdown,
}

impl WalEngine {
    pub async fn start_dummy() -> Arc<Self> {
        let (tx, _) = mpsc::channel::<OpLog>(100);
        let (cmd_tx, _) = mpsc::channel::<WalCommand>(10);

        let index = Arc::new(RwLock::new(BTreeMap::new()));

        Arc::new(Self {
            sender: tx,
            cmd_sender: cmd_tx,
            index,
        })
    }

    /// Bootstraps the crash-safe WAL with automatic torn-frame recovery
    pub async fn start(file_path: String) -> (Arc<Self>, tokio::task::JoinHandle<()>) {
        println!("Bismillah. Booting Portable Nucleus Crash-Safe WAL Engine...");

        // Pre-Flight Forensic Scan - Detect and trunate torn frames from prior crashes.
        let (clean_offset, recovered_index) = Self::recover_and_truncate_torn_frames(&file_path);

        // Bounded channel to prevent OOM crashes
        let (tx, mut rx) = mpsc::channel::<OpLog>(100_000);
        let (cmd_tx, mut cmd_rx) = mpsc::channel::<WalCommand>(10);

        let index = Arc::new(RwLock::new(recovered_index));
        let index_clone = index.clone();
        let fp_clone = file_path.clone();

        // Spawn Tokio Worker task
        let handle = tokio::spawn(async move {
            let mut active_file = tokio::fs::OpenOptions::new()
                .create(true)
                .read(true)
                .write(true)
                .append(true)
                .open(&file_path)
                .await
                .expect("Failed to open crash-safe WAL file");

            // Explicit hardware seek to guarantee alignment with recovered index.
            active_file
                .seek(std::io::SeekFrom::Start(clean_offset))
                .await
                .expect("FATAL: Failed to seek WAL writes cursor to clean_offset");

            let mut current_offset = clean_offset;
            let mut batch: Vec<OpLog> = Vec::with_capacity(6_000);

            // Group commit timer (flushes the disk cache every 2ms if data is pending)
            let mut flush_interval = interval(Duration::from_millis(2));

            loop {
                tokio::select! {
                    // Path A: Incoming log item
                    msg = rx.recv() => {
                        match msg  {
                            Some(log) => {
                              batch.push(log);

                            // Drain the channel of any other pending thoughts for batching
                            while batch.len() < 6_000 {
                                    if let Ok(pending_log) = rx.try_recv() {
                                        batch.push(pending_log);
                                    } else {
                                        break;
                                    }
                            }

                            if !batch.is_empty() {
                                Self::write_batch_to_disk(&mut active_file, &mut current_offset, &batch, &index_clone).await;

                                batch.clear();
                            }

                    }
                            None => break

                        }

                    }

                    // Path B: Group Commit Flush (Enforces hardware NVMe cache sync)
                    _ = flush_interval.tick() => {
                        let _ = active_file.sync_data().await;
                    }

                    // Path C: Segment Rotation Command
                    cmd = cmd_rx.recv() => {
                        match cmd {
                        Some(WalCommand::Shutdown) => {
                            let _ = active_file.sync_all().await;
                            break;
                        }

                        Some(WalCommand::Rotate(reply_tx)) => {
                            println!("[WAL_ENGINE] Halting I/O. Rotating WAL segment...");

                            // 1. Force final hardware flush
                            let _ = active_file.sync_data().await;

                            // Generate archived filename based on unix timestamp
                            let timestamp = SystemTime::now().duration_since(UNIX_EPOCH).unwrap().as_millis();
                            let archived_name = format!("{}_{}.wal", fp_clone, timestamp);

                            // Async File rename (non-blocking)
                            if let Err(e) = tokio::fs::rename(&fp_clone, &archived_name).await {
                                eprintln!("[WAL_ENGINE ERROR] Rotation rename failed: {} ", e);
                                continue;
                            }

                            // Open a fresh active file & reset offset
                            active_file = tokio::fs::OpenOptions::new().create(true).read(true).write(true).open(&fp_clone).await.unwrap();
                            current_offset = 0;

                            // Clear memory index for fresh segment
                            {
                                let mut idx = index_clone.write().await;
                                idx.clear();
                            }

                            // Tell the compactor the achived file is ready
                            let _ = reply_tx.send(archived_name);
                            println!("[WAL_ENGINE] Rotation complete. I/O resumed.");

                    }

                            None => break,
                        }

                    }

                }
            }
        });

        (
            Arc::new(Self {
                sender: tx,
                cmd_sender: cmd_tx,
                index,
            }),
            handle,
        )
    }

    /// Internal Helper: Checksummed Serialization, Non-blocking Write, and Hardware Sync Barrier
    async fn write_batch_to_disk(
        file: &mut tokio::fs::File,
        current_offset: &mut u64,
        batch: &[OpLog],
        index: &Arc<RwLock<BTreeMap<u128, u64>>>,
    ) {
        if batch.is_empty() {
            return;
        }
        let first_txid = batch[0].state.transaction_id;

        // zero-copy serialize the entire batch
        let payload_bytes = to_bytes::<rkyv::rancor::Error>(&batch.to_vec())
            .expect("Failed to serialize batch")
            .into_vec();

        // Compute CRC32 checksums over payload bytes
        let payload_len = payload_bytes.len() as u32;
        let checksum = crc32fast::hash(&payload_bytes);

        let len_prefix = payload_len.to_le_bytes();
        let crc_bytes = checksum.to_le_bytes();

        // Sequential write using tokio async I/O
        if let Err(e) = file.write_all(&len_prefix).await {
            eprintln!("[WAL_ENGINE FATAL] Length prefix write failed: {}", e);
            return;
        }

        if let Err(e) = file.write_all(&crc_bytes).await {
            eprintln!("[WAL_ENGINE FATAL] CRC32 checksum write failed: {} ", e);
            return;
        }

        if let Err(e) = file.write_all(&payload_bytes).await {
            eprintln!(" [WAL_ENGINE FATAL] Payload write failed: {}", e);
            return;
        }

        // Hardware sync Sync Barrier: Issue fdatasync hardware flush NVMe controller DRAM cache
        if let Err(e) = file.sync_all().await {
            eprintln!(
                "[WAL_ENGIME CRITICAL] fdatasync hardware flush failed: {}",
                e
            );
            return;
        }

        // Update in-memory index after hardware persisten is confirmed
        {
            let mut idx = index.write().await;
            idx.insert(first_txid, *current_offset);
        }

        *current_offset += 4 + 4 + payload_bytes.len() as u64;
    }

    /// Pre-flight recovery: Scans WAL on boot, validates crc32 checksums and truncates torn tail frames
    pub fn recover_and_truncate_torn_frames(file_path: &str) -> (u64, BTreeMap<u128, u64>) {
        let mut file = match File::options().read(true).write(true).open(file_path) {
            Ok(f) => f,
            Err(_) => {
                println!(
                    "[WAL] No existing WAL file at {}. Initializing fresh 0-byte log. ",
                    file_path
                );
                return (0, BTreeMap::new());
            }
        };

        let mut offset: u64 = 0;
        let mut index = BTreeMap::new();
        let mut len_buf = [0u8; 4];
        let mut crc_buf = [0u8; 4];

        println!(
            "[WAL RECOVERY] Validating frame checksums on {}...",
            file_path
        );

        loop {
            // Read 4-byte length prefix
            if file.read_exact(&mut len_buf).is_err() {
                break;
            }

            let payload_len = u32::from_le_bytes(len_buf) as usize;

            // Read 4-byte CRC32 checksums
            if file.read_exact(&mut crc_buf).is_err() {
                eprintln!(
                    "[WAL RECOVERY WARN] Truncated CRC header at offset {}. Truncating tail. ",
                    offset
                );
                break;
            }
            let expected_crc = u32::from_le_bytes(crc_buf);

            // Read payload bytes
            let mut payload = vec![0u8; payload_len];
            if file.read_exact(&mut payload).is_err() {
                eprintln!(
                    "[WAL RECOVERY WARN] Truncated payload at offset {}. Power failure detected. Truncating tail.",
                    offset
                );
                break;
            }

            // Validate CRC32 Checksum
            let computed_crc = crc32fast::hash(&payload);
            if computed_crc != expected_crc {
                eprintln!(
                    "[WAL RECOVERY CORRUPTION] CRC mismatch at offset {}! Expected {:x}, computed {:x}. Truncating torn write.",
                    offset, expected_crc, computed_crc
                );

                break;
            }

            // Zero-copy rkyv exraction to build memory index
            if let Ok(archived_batch) = rkyv::access::<
                <Vec<OpLog> as rkyv::Archive>::Archived,
                rkyv::rancor::Error,
            >(&payload)
            {
                if let Ok(batch) =
                    rkyv::deserialize::<Vec<OpLog>, rkyv::rancor::Error>(archived_batch)
                {
                    if !batch.is_empty() {
                        index.insert(batch[0].state.transaction_id, offset);
                    }
                }
            }

            offset += 4 + 4 + payload_len as u64;
        }

        // Truncate file back to the last 100% valid checksum-verified boundary
        if let Err(e) = file.set_len(offset) {
            eprintln!(
                "[WAL RECOVERY ERROR] Failed to set file length during truncation: {}",
                e
            );
        } else {
            println!(
                "[WAL RECOVERY COMPLETE] Clean WAL tail established at offset {} bytes. {} valid batches indexed",
                offset,
                index.len()
            );
        }

        (offset, index)
    }

    /// Returns the number of uncompacted logs currently in the Hot WAL. O(1) operation utilizing the BTreeMap index
    pub async fn get_pending_count(&self) -> usize {
        self.index.read().await.len()
    }

    /// Fire and forget. The TCP/Agent networking layer NEVER blocks here.
    pub async fn append(&self, log: OpLog) -> Result<(), WalError> {
        self.sender
            .send(log)
            .await
            .map_err(|e| WalError::IngressQueueFull(e.to_string()))
    }

    /// Extremely fast binary scan of the active WAL for a specific substring
    pub fn lexical_scan(
        &self,
        query: &str,
        namespace_filter: Option<&str>,
        limit: usize,
        wal_path: &str,
    ) -> Result<Vec<VaultSearchResult>, anyhow::Error> {
        let file = File::open(wal_path)?;
        let mmap = unsafe { MmapOptions::new().map(&file)? };

        let ac = AhoCorasick::builder()
            .ascii_case_insensitive(true)
            .build(vec![query])
            .map_err(|e| anyhow::anyhow!("Failed to build automaton: {}", e))?;

        let mut results = Vec::new();
        let mut cursor = 0;

        while cursor + 8 <= mmap.len() && results.len() < limit {
            let len = u32::from_le_bytes(mmap[cursor..cursor + 4].try_into().unwrap()) as usize;
            let expected_crc = u32::from_le_bytes(mmap[cursor + 4..cursor + 8].try_into().unwrap());
            cursor += 8; // FIXED: Advance past [4B Length] + [4B CRC32]

            if cursor + len > mmap.len() {
                break;
            }
            let payload = &mmap[cursor..cursor + len];
            cursor += len;

            if crc32fast::hash(payload) != expected_crc {
                continue;
            }

            // TRUE ZERO-COPY: Inspects Vec<OpLog> pointer directly in kernel mmap page
            if let Ok(archived_batch) = rkyv::access::<
                <Vec<OpLog> as rkyv::Archive>::Archived,
                rkyv::rancor::Error,
            >(payload)
            {
                for archived_log in archived_batch.as_slice() {
                    let text = archived_log.state.text.as_str();
                    let ns = archived_log.state.namespace.as_str();

                    if let Some(filter) = namespace_filter {
                        if !filter.is_empty() && filter != ns {
                            continue;
                        }
                    }

                    if ac.is_match(text) {
                        results.push(VaultSearchResult {
                            agent_hex: hex::encode(archived_log.agent_id.as_slice()),
                            tx_id: archived_log.state.transaction_id.to_native(),
                            namespace: ns.to_string(),
                            payload: text.to_string(),
                            timestamp: archived_log.state.timestamp.to_string(),
                            source: "HOT_WAL".to_string(),
                            similarity_score: 1.0,
                        });

                        if results.len() >= limit {
                            break;
                        }
                    }
                }
            }
        }

        results.reverse();
        Ok(results)
    }

    pub fn fetch_hot_timeline(
        &self,
        agent_hex: &str,
        wal_path: &str,
    ) -> Result<Vec<TimelineNode>, anyhow::Error> {
        let mut nodes = Vec::new();

        let file = match File::open(wal_path) {
            Ok(f) => f,
            Err(_) => return Ok(nodes),
        };

        // Page WAL into memory
        let mmap = unsafe { MmapOptions::new().map(&file)? };
        let mut cursor = 0;
        let target_bytes = hex::decode(agent_hex).unwrap_or(vec![0; 16]);

        while cursor + 8 <= mmap.len() {
            let len = u32::from_le_bytes(mmap[cursor..cursor + 4].try_into().unwrap()) as usize;
            let expected_crc = u32::from_le_bytes(mmap[cursor + 4..cursor + 8].try_into().unwrap());
            cursor += 8;

            if cursor + len > mmap.len() {
                break;
            }

            let payload = &mmap[cursor..cursor + len];
            cursor += len;

            if crc32fast::hash(payload) != expected_crc {
                continue;
            }

            if let Ok(archived_batch) = rkyv::access::<
                <Vec<OpLog> as rkyv::Archive>::Archived,
                rkyv::rancor::Error,
            >(payload)
            {
                for archived_log in archived_batch.as_slice() {
                    if archived_log.agent_id.as_slice() == target_bytes.as_slice() {
                        let status_str = match &archived_log.state.status {
                            rkyv::Archived::<AgentStatus>::Idle => "Idle",
                            rkyv::Archived::<AgentStatus>::Reasoning => "Reasoning",
                            rkyv::Archived::<AgentStatus>::Halted => "Halted",
                            rkyv::Archived::<AgentStatus>::ToolExecution => "ToolExecution",
                        };

                        nodes.push(TimelineNode {
                            tx_id: archived_log.state.transaction_id.to_native(),
                            timestamp: archived_log.state.timestamp.to_string(),
                            agent_status: status_str.to_string(),
                            payload_preview: archived_log.state.text.as_str().to_string(),
                        });
                    }
                }
            }
        }

        Ok(nodes)
    }

    /// Scans the raw WAL file to find the highest TxID it contains.
    /// Executes syncronously during the OS Bootstrap phase.
    pub fn get_highest_tx_id(&self, file_path: &str) -> u128 {
        let mut file = match std::fs::File::open(file_path) {
            Ok(f) => f,
            Err(_) => {
                println!(
                    "[WAL] No existing WAL found at {}. Starting fresh. ",
                    file_path
                );
                return 0;
            }
        };

        let mut highest_tx: u128 = 0;
        let mut len_buf = [0u8; 4];
        let mut crc_buf = [0u8; 4];

        // Iterate throught the append-only binary log.
        while file.read_exact(&mut len_buf).is_ok() {
            let payload_len = u32::from_le_bytes(len_buf) as usize;

            if file.read_exact(&mut crc_buf).is_err() {
                break;
            }

            let mut payload = vec![0u8; payload_len];

            if file.read_exact(&mut payload).is_err() {
                eprintln!("[WAL WARNING] Corrupted trailing bytes detected. Truncation required. ");
                break;
            }

            // Bounds-checked zero-copy pointer access
            if let Ok(batch) = rkyv::access::<
                <Vec<OpLog> as rkyv::Archive>::Archived,
                rkyv::rancor::Error,
            >(&payload)
            {
                for archived_log in batch.as_slice() {
                    let tx_id = archived_log.state.transaction_id.to_native();

                    if tx_id > highest_tx {
                        highest_tx = tx_id;
                    }
                }
            }
        }

        highest_tx
    }
}
