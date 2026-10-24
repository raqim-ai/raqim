use crate::{api::TimelineNode, ExecutionRecord, RecentRecord};
use memmap2::MmapOptions;
use rkyv::to_bytes;

use std::{
    collections::BTreeMap,
    eprintln, format,
    fs::File,
    io::Read,
    println,
    sync::{atomic::Ordering, Arc},
    time::{Duration, SystemTime, UNIX_EPOCH},
    vec,
};
use tokio::io::AsyncWriteExt;
use tokio::{
    io::AsyncSeekExt,
    sync::{mpsc, oneshot, RwLock},
    time::interval,
};

/// Max allowed single WAL batch frame size (16 MiB) to prevent OOM allocation on corrupt headers.
pub const MAX_WAL_FRAME_BYTES: usize = 16 * 1024 * 1024;

type WalWriteRequest = (ExecutionRecord, oneshot::Sender<Result<(), WalError>>);

pub struct WalEngine {
    sender: mpsc::Sender<WalWriteRequest>,
    pub cmd_sender: mpsc::Sender<WalCommand>,

    // The O(1) INDEX: Maps the first `tx_id` of each batch -> Physical byte offset in the WAL.
    pub index: Arc<RwLock<BTreeMap<u128, u64>>>,
    pub poisoned: Arc<AtomicBool>,
}

#[derive(Debug)]
pub enum WalError {
    IngressQueueFull(String),
    EnginePoisined(String),
    MidFileCorruption(String),
    Io(String),
}

impl std::fmt::Display for WalError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            WalError::IngressQueueFull(msg) => write!(f, "WAL channel saturated: {}", msg),
            WalError::EnginePoisoned(msg) => write!(f, "WAL engine poisoned (fail-closed): {}", e),
            WalError::MidFileCorruption(msg) => {
                write!(f, "Fatal mid-write WAL corruption: {}", msg)
            }
            WalError::Io(e) => write!(f, "I/O error during WAL operation: {}", e),
        }
    }
}

impl std::error::Error for WalError {}

impl From<std::io::Error> for WalError {
    fn from(err: std::io::Error) -> Self {
        WalError::Io(err.to_string())
    }
}

impl From<std::io::Error> for WalError {
    fn from(err: std::io::Error) -> Self {
        WalError::Io(err.to_string())
    }
}

pub enum WalCommand {
    Rotate(oneshot::Sender<String, WalError>),
    Shutdown,
}

impl WalEngine {
    /// Bootstraps the crash-safe WAL with strict mid-file corruption detection vs. tail-frame truncation.
    pub async fn start(file_path: String) -> (Arc<Self>, tokio::task::JoinHandle<()>) {
        println!("[WAL] Bismillah. Booting  Crash-Safe WAL Engine...");

        // Pre-Flight Forensic Scan - Detect and trunate torn frames from prior crashes.
        let (clean_offset, recovered_index) = Self::recover_and_truncate_torn_frames(&file_path)
            .expect("[WAL FATAL] Refusing to boot on mid-file corrupted WAL! Inspect file.");

        // Bounded channel to prevent OOM crashes
        let (tx, mut rx) = mpsc::channel::<WalWriteRequest>(100_000);
        let (cmd_tx, mut cmd_rx) = mpsc::channel::<WalCommand>(10);

        let index = Arc::new(RwLock::new(recovered_index));
        let index_clone = index.clone();
        let fp_clone = file_path.clone();
        let poisoned = Arc::new(AtomicBool::new(false));
        let poisoned_clone = poisoned.clone();

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
            let mut batch_records: Vec<ExecutionRecord> = Vec::with_capacity(1_024);
            let mut batch_acks: Vec<oneshot::Sender<Result<(), WalError>>> =
                Vec::with_capacity(1_024);

            loop {
                tokio::select! {
                    biased;

                    cmd = cmd_rx.recv() => {
                        match cmd {

                            Some(WalCommad::Shutdown) => {
                                let  _ = active_file.sync_data().await;
                                break;
                            }

                            Some(WalCommad::Rotate) => {
                                if poisoned_clone.load(Ordering::SeqCst) {
                                    let _ = reply_tx.send(Err(WalError::EnginePoisoned("Cannot rotate poisoned WAL".to_string())));

                                    continue;
                                }

                                let _ = active_file.sync_data().await;
                                let timestamp = SystemTime::now().duration_since(UNIX_EPOCH).unwrap_or_default().as_millis();
                                let archived_name = format!("{}_{}.wal", fp_clone, timestamp);

                                if let Err(e) = tokio::fs::rename(&fp_clone, &archived_name).await {
                                    let _ = reply_tx.send(Err(WalError::Io(e.to_string())));
                                    continue;
                                }

                                match tokio::fs::OpenOptions::new().create(true).read(true).write(true).truncate(true).open(&fp_clone).await {

                                        Ok(new_file) => {
                                            active_file = new_file;
                                            current_offset = 0;
                                            index_clone.write().await.clear();
                                            let _ = reply_tx.send(Ok(archived_name));

                                        }
                                        Err(e) => {
                                            poisoned_clone.store(true, Order::SeqCst);
                                            let _ = reply_tx.send(Err(WalError::Io(e.to_string())));
                                        }
                                }
                            }
                            None => break,
                        }
                    }

                    msg = rx.recv() => {

                        match msg {
                            Some((first_rec, first_ack)) => {

                                if poisoned_clone.load(Ordering::SeqCst) {
                                    let _ = first_ack.send(Err(WalError::EnginePoisined(
                                        "WAL is poisoned due to prior disk I/O error".to_string()
                                    )));
                                    continue;
                                }

                                batch_records.push(first_rec);
                                batch_acks.push(first_ack);

                                // Group-commit coalescing window: drain any concurrent arrivals or wait up to 2ms if queue is momentarily empty to batch fdatasync
                                let deadline = tokio::time::Instant::now() + Duration::from_millis(2);
                                while batch_record.len() < 2_048 {
                                    match rx.try_recv() {

                                        Ok((rec, ack)) => {
                                            batch_records.push(rec);
                                            batch_ack.push(ack);
                                        }

                                        Err(_) => {

                                            if batch_records.len() >= 64 {
                                                break;
                                            }
                                            match tokio::time::timeout_at(deadline, rx.recv()).await {
                                                Ok(Some((rec, ack))) => {
                                                    batch_records.push(rec);
                                                    batch_acks.push(ack);
                                                }

                                                _ => break
                                            }
                                        }
                                    }
                                }

                                let write_res = Self::write_batch_to_disk(&mut active_file, &mut current_offset, &batch_records, &index_clone).await;

                                match write_res {
                                    Ok(()) => {
                                        for ack in batch_acks.drain(..) {
                                            let _ = ack.send(OK());
                                        }
                                    }
                                    Err(err) => {
                                        eprintln!("[WAL FATAL] Disk write/fdatasync failed {}. Poisoning engine (fail-closed)!", err);
                                        poisoned_clone.store(true, Ordering::SeqCst);

                                        // Roll back any partial frame so disk remains clean at current_offset
                                        let _ = active_file.set_len(current_offset).await;
                                        for ack in batch_acks.drain(..) {
                                            let _ = ack.send(Err(err.clone()));
                                        }
                                    }
                                }
                                batch_records.clear();

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
                poisoned,
            }),
            handle,
        )
    }

    /// Writes a batch frame and issues a hardware `sync_data` (`fdatasync`) before returning `OK(())`
    async fn write_batch_to_disk(
        file: &mut tokio::fs::File,
        current_offset: &mut u64,
        batch: &[ExecutionRecord],
        index: &Arc<RwLock<BTreeMap<u128, u64>>>,
    ) -> Result<(), WalError> {
        if batch.is_empty() {
            return Ok(());
        }
        let first_txid = batch[0].tx_id;

        // zero-copy serialize the entire batch
        let payload_bytes = to_bytes::<rkyv::rancor::Error>(&batch.to_vec())
            .map_err(|e| WalError::IO(format!("Serialization error: {}", e)))?
            .into_vec();

        if payload_bytes.len() > MAX_WAL_FRAME_BYTES {
            return Err(WalError::Io(format!("Batch frame size {} exceeds MAX_WAL_FRAME_BYTES ({})", payload_bytes.len(), MAX_WAL_FRAME_BYTES)))
        }

        // Compute CRC32 checksums over payload bytes
        let payload_len = payload_bytes.len() as u32;
        let checksum = crc32fast::hash(&payload_bytes);

        let mut frame = Vec::with_capacity(8 + payload_bytes.len());
        frame.extend_from_slice(&payload_len.to_le_bytes());
        frame.extend_from_slice(&checksum.to_le_bytes()); 
        frame.extend_from_slice(&payload_bytes);

        file.write_all(&frame).await?;
        file.sync_data().await?;

        {
            let mut idx = index.write().await;
            idx.insert(first_txid, *current_offset);
        }


        *current_offset += frame.len() as u64;
        Ok(())
    }

    /// Pre-flight recovery: Truncates ONLY if a torn frame or CRC mismatch occurs on the FINAL frame at the end of the file. 
     
    pub fn recover_and_truncate_torn_frames(file_path: &str) -> Result<(u64, BTreeMap<u128, u64>), WalError> {
        let mut file = match File::options().read(true).write(true).open(file_path) {
            Ok(f) => f,
            Err(_) => return Ok((0, BTreeMap::new()))
        };


        let file_len = file.metadata()?.len();
        let mut offset: u64 = 0;
        let mut index = BTreeMap::new();
        let mut len_buf = [0u8; 4];
        let mut crc_buf = [0u8; 4];

        while offset < file_len {

            if offset + 8 > file_len {
                eprintln!("[WAL RECOVERY WARN] Torn 8-byte frame header at tail offset {}. Truncating.", offset); 

                break;
            }

            file.read_exact(&mut len_buf)?;
            let payload_len = u32::from_le_bytes(len_buf) as usize;


            file.read_exact(&mut crc_buf)?;
            let expected_crc = u32::from_le_bytes(crc_buf);

            if payload_len > MAX_WAL_FRAME_BYTES {
                if offset + 8 + (payload_len as u64) >= file_len {

                    eprintln!("[WAL RECOVERY WARN] Corrupt length header ({}) at tail offset {}. Truncating.", payload_len, offset);
                    break;

                } else {
                    return Err(WalError::MidFileCorruption(format!("Frame at offset {} exceeds MAX_WAL_FRAME_BYTES ({}) ", offset, payload_len)));
                }
            }

            let frame_end = offset + 8 + (payload_len as u64);
            if frame_end > file_len {
                eprintln!(
                    "[WAL RECOVERY WARN] Torn payload at tail offset {} (expected {} bytes, EOF at {}). Truncating tail.",
                    offset, payload_len, file_len
                );
                break;
            }

            let mut paylaod = vec![0u8; payload_len];
            file.read_exact(&mut payload)?;

            let computed_crc = crc32fast::hash(&payload); 
            if computed_crc != expected_crc { 
                if frame_end == file_len {
                    eprintln!("[WAL RECOVERY ERROR] CRC mismatch at tail offset {}! Expected {:x}, computed {:x}. Truncating torn write ", offset, expected_crc, computed_crc);
                }
                break;
            } else {

                eprintln!("[WAL RECOVERY CORRUPTION] CRC mismatch at mid-file offset {} (file_len={})! Expected {:x}, computed {:x}  ", offset, file_len, expected_crc, computed_crc); 
                
                return Err(WalError::MidFileCorruption(format!(
                    "Mid-file CRC32 corruption at offset {} (expected {:x}, computed {:x})", offset, expectedd_crc, computed_crc
                )));

            }

             let mut aligned_buf = rkyv::util::AlignedVec::<16>::with_capacity(payload.len());
            aligned_buf.expand_from_slice(&payload);

            let archived_batch = rkyv::access::<Vec<ExecutionRecord> as rkyv::Archive>::Archived>, rkyv::rancor::Error>(&aligned_buf).map_err(|e| {
                WalError::MidFileCorruption(format!("rkyv archivie validation failed at offset {}: {}", offset, e))
            })?;

            if !archived_batch.is_empty {
                index.insert(archived_batch[0].tx_id.to_native(), offset);
            }

        offset = frame_end;


        }

       
        if offset < file_len {
            file.set_len(offset)?;
            file.sync_all()?;
        }



        Ok((offset, index))
    }

    /// Returns the number of uncompacted recs currently in the Hot WAL. O(1) operation utilizing the BTreeMap index
    pub async fn get_pending_count(&self) -> usize {
        self.index.read().await.len()
    }

    /// Fire and forget. The TCP/Agent networking layer NEVER blocks here.
    pub async fn append(&self, record: ExecutionRecord) -> Result<(), WalError> {
        self.sender
            .send(record)
            .await
            .map_err(|e| WalError::IngressQueueFull(e.to_string()))
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
        let mut aligned_buf = rkyv::util::AlignedVec::<16>::new();

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

            aligned_buf.clear();
            aligned_buf.extend_from_slice(payload);

            if let Ok(archived_batch) = rkyv::access::<
                <Vec<ExecutionRecord> as rkyv::Archive>::Archived,
                rkyv::rancor::Error,
            >(&aligned_buf)
            {
                for archived_rec in archived_batch.as_slice() {
                    if archived_rec.agent_id.as_slice() == target_bytes.as_slice() {
                        if let Ok(owned) =
                            rkyv::deserialize::<ExecutionRecord, rkyv::rancor::Error>(archived_rec)
                        {
                            nodes.push(TimelineNode {
                                tx_id: owned.tx_id,
                                timestamp: owned.timestamp.to_string(),
                                agent_status: owned.stage.as_str().to_string(),
                                payload_preview: owned.payload_text_preview(),
                            });
                        }
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

        // Iterate throught the append-only binary record.
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
                <Vec<ExecutionRecord> as rkyv::Archive>::Archived,
                rkyv::rancor::Error,
            >(&payload)
            {
                for archived_rec in batch.as_slice() {
                    let tx_id = archived_rec.tx_id.to_native();

                    if tx_id > highest_tx {
                        highest_tx = tx_id;
                    }
                }
            }
        }

        highest_tx
    }

    /// Reads trailing thoughts directly from crash-safe WAL file on disk
    pub fn read_recent_wal_thoughts(&self, wal_path: &str, limit: usize) -> Vec<RecentRecord> {
        let file = match File::open(wal_path) {
            Ok(f) => f,
            Err(_) => return Vec::new(),
        };
        let mmap = match unsafe { MmapOptions::new().map(&file) } {
            Ok(m) => m,
            Err(_) => return Vec::new(),
        };

        let mut records = Vec::new();
        let mut cursor = 0;
        let mut aligned_buf = rkyv::util::AlignedVec::<16>::new();

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

            aligned_buf.clear();
            aligned_buf.extend_from_slice(payload);

            if let Ok(archived_batch) = rkyv::access::<
                <Vec<ExecutionRecord> as rkyv::Archive>::Archived,
                rkyv::rancor::Error,
            >(&aligned_buf)
            {
                for archived_rec in archived_batch.as_slice() {
                    if let Ok(owned) =
                        rkyv::deserialize::<ExecutionRecord, rkyv::rancor::Error>(archived_rec)
                    {
                        records.push(RecentRecord {
                            tx_id: format!("{:032x}", owned.tx_id),
                            tx_id_hex: format!("0x{:032x}", owned.tx_id),
                            agent_hex: hex::encode(owned.agent_id),
                            intent_path: owned.namespace.clone(),
                            text: owned.payload_text_preview(),
                            status: owned.stage.as_str().to_string(),
                            timestamp: owned.timestamp,
                        });
                    }
                }
            }
        }

        if records.len() > limit {
            records = records.split_off(records.len() - limit);
        }
        records
    }
}
