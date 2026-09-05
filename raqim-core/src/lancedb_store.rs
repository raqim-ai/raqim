use crate::api::{TimelineNode, VaultSearchResult};
use crate::embedding::EmbeddingProvider;
use crate::{OpLog, SystemEvent};
use arrow_array::types::Float32Type;
use arrow_array::{Array, Float32Array};
use arrow_array::{
    BinaryArray, FixedSizeListArray, Int64Array, RecordBatch, RecordBatchIterator, StringArray,
};
use arrow_schema::{DataType, Field, Schema};
use futures::StreamExt;
use lancedb::connect;
use lancedb::connection::Connection;
use lancedb::query::ExecutableQuery;
use lancedb::query::QueryBase;
use std::collections::HashMap;
use std::format;
use std::path::Path;
use std::sync::Arc;
use std::time::{SystemTime, UNIX_EPOCH};

pub struct LanceEngine {
    pub db: Connection,
    pub history_table: String,
    pub snapshot_table: String,
    pub storage_path: String,
    pub dims: i32,
    pub embedder: Arc<dyn EmbeddingProvider>,
}

#[derive(Clone, Debug)]
pub struct ColdSearchResult {
    pub tx_id: u128,
    pub agent_hex: String,
    pub status: String,
    pub text: String,
    pub namespace: String,
    pub timestamp: i64,

    pub distance: f32,
}

impl LanceEngine {
    pub async fn new(
        storage_path: &str,
        table_name: &str,
        embedder: Arc<dyn EmbeddingProvider>,
    ) -> Self {
        println!("Bismillah. Booting LanceEngine & Local Embedding Model...");
        let db = connect(storage_path)
            .execute()
            .await
            .expect("Failed to connect to lanceDB");
        let dims = embedder.dimension();

        Self {
            db,
            history_table: table_name.to_string(),
            storage_path: storage_path.to_string(),
            snapshot_table: "agent_snapshot".to_string(),
            embedder,
            dims,
        }
    }

    pub async fn new_dummy(embedder: Arc<dyn EmbeddingProvider>) -> Self {
        let db = connect("dummy_lance_path")
            .execute()
            .await
            .expect("Failed to conenct to dummy LanceDB ");

        Self {
            db,
            history_table: "dummy_history".to_string(),
            snapshot_table: "dummy_snapshot".to_string(),
            storage_path: "dummy_storage".to_string(),
            embedder,
            dims: 768,
        }
    }

    /// Executes Cold vector Prosimity search using a pre-computed query vector
    pub async fn search_cold_vector(
        &self,
        query_vector: &[f32],
        namespace_filter: Option<&str>,
        limit: usize,
    ) -> Result<Vec<ColdSearchResult>, anyhow::Error> {
        let table = self.db.open_table(&self.history_table).execute().await?;

        let mut query_builder = table.query().nearest_to(query_vector)?;

        if let Some(ns) = namespace_filter {
            if !ns.is_empty() {
                query_builder = query_builder.only_if(format!("namespace = '{}'", ns));
            }
        }

        let mut stream = query_builder.limit(limit).execute().await?;
        let mut results = Vec::new();

        while let Some(batch_result) = stream.next().await {
            let batch = batch_result?;

            let tx_col = batch
                .column_by_name("tx_id")
                .unwrap()
                .as_any()
                .downcast_ref::<StringArray>()
                .unwrap();
            let agent_id_col = batch
                .column_by_name("agent_id")
                .unwrap()
                .as_any()
                .downcast_ref::<StringArray>()
                .unwrap();
            let namespace_col = batch
                .column_by_name("namespace")
                .unwrap()
                .as_any()
                .downcast_ref::<StringArray>()
                .unwrap();
            let status_col = batch
                .column_by_name("status")
                .unwrap()
                .as_any()
                .downcast_ref::<StringArray>()
                .unwrap();
            let text_col = batch
                .column_by_name("text")
                .unwrap()
                .as_any()
                .downcast_ref::<StringArray>()
                .unwrap();
            let timestamp_col = batch
                .column_by_name("timestamp")
                .unwrap()
                .as_any()
                .downcast_ref::<Int64Array>()
                .unwrap();

            for i in 0..tx_col.len() {
                let parsed_tx = u128::from_str_radix(tx_col.value(i), 16).unwrap_or(0);
                results.push(ColdSearchResult {
                    tx_id: parsed_tx,
                    agent_hex: agent_id_col.value(i).to_string(),
                    namespace: namespace_col.value(i).to_string(),
                    status: status_col.value(i).to_string(),
                    text: text_col.value(i).to_string(),
                    timestamp: timestamp_col.value(i),

                    distance: 0.0,
                });
            }
        }

        Ok(results)
    }

    /// The Semantic Retriever. Now returns a structured UI data, not a raw string.
    pub async fn semantic_search(
        &self,
        query: &str,
        namespace_filter: Option<&str>,
        limit: usize,
    ) -> Result<Vec<VaultSearchResult>, anyhow::Error> {
        // Math translation via the polymorphic embedder
        let query_vector = self.embedder.embed(query).await?;

        let table = self.db.open_table(&self.history_table).execute().await?;

        // Build the query dynamically based on namespace constraints
        let mut query_builder = table.query().nearest_to(query_vector)?;
        if let Some(ns) = namespace_filter {
            if !ns.is_empty() {
                query_builder = query_builder.only_if(format!("namespace = '{}'", ns));
            }
        }

        let mut stream = query_builder.limit(limit).execute().await?;
        let mut results = Vec::new();

        while let Some(batch_result) = stream.next().await {
            let batch = batch_result?;

            // We must exttract the hidden "_distance" column that LanceDB generates during `nearest_to` searches
            let text_col = batch
                .column_by_name("text")
                .unwrap()
                .as_any()
                .downcast_ref::<StringArray>()
                .unwrap();
            let agent_id_col = batch
                .column_by_name("agent_id")
                .unwrap()
                .as_any()
                .downcast_ref::<StringArray>()
                .unwrap();
            let ns_col = batch
                .column_by_name("namespace")
                .unwrap()
                .as_any()
                .downcast_ref::<StringArray>()
                .unwrap();
            let timestamp_col = batch
                .column_by_name("timestamp")
                .unwrap()
                .as_any()
                .downcast_ref::<Int64Array>()
                .unwrap();
            let tx_id_col = batch
                .column_by_name("tx_id")
                .unwrap()
                .as_any()
                .downcast_ref::<StringArray>()
                .unwrap();
            let dist_col = batch
                .column_by_name("_distance")
                .unwrap()
                .as_any()
                .downcast_ref::<Float32Array>()
                .unwrap();

            for i in 0..text_col.len() {
                // Cosing distance to Similarity mapping (1.0 - distance)
                let similatiry = 1.0 - dist_col.value(i);

                results.push(VaultSearchResult {
                    tx_id: u128::from_str_radix(&tx_id_col.value(i).to_string().as_str(), 16)
                        .unwrap_or(0),
                    agent_hex: agent_id_col.value(i).to_string(),
                    namespace: ns_col.value(i).to_string(),
                    source: "LANCEDB".to_string(),
                    similarity_score: similatiry,
                    payload: text_col.value(i).to_string(),
                    timestamp: timestamp_col.value(i).to_string(),
                });
            }
        }

        Ok(results)
    }

    /// The exact Apache Arrow Schema mapping for our OpLog
    fn schema(&self) -> Arc<Schema> {
        Arc::new(Schema::new(vec![
            Field::new("tx_id", DataType::Utf8, false),
            Field::new("agent_id", DataType::Utf8, false),
            Field::new("namespace", DataType::Utf8, false),
            Field::new("timestamp", DataType::Int64, false),
            Field::new("status", DataType::Utf8, false),
            Field::new("text", DataType::Utf8, false),
            Field::new("entropy_seeds", DataType::Utf8, false),
            Field::new("network_responses", DataType::Utf8, false),
            // We store the raw binary delta
            Field::new("payload", DataType::Binary, false),
            Field::new(
                "vector",
                DataType::FixedSizeList(
                    Arc::new(Field::new("item", DataType::Float32, true)),
                    self.dims,
                ),
                false,
            ),
        ]))
    }

    /// Schema for WASM memory snapshot
    fn snapshot_schema(&self) -> Arc<Schema> {
        Arc::new(Schema::new(vec![
            Field::new("tx_id", DataType::Utf8, false),
            Field::new("timestamp", DataType::Int64, false),
            Field::new("agent_id", DataType::Utf8, false),
            Field::new("memory_blob", DataType::Binary, false),
        ]))
    }

    /// The Unified Schema for all Systems and Security Events.
    fn audit_schema(&self) -> Arc<Schema> {
        Arc::new(Schema::new(vec![
            Field::new("timestamp", DataType::Int64, false),
            Field::new("event_type", DataType::Utf8, false),
            Field::new("agent_id", DataType::Utf8, false),
            Field::new("metadata", DataType::Utf8, false),
        ]))
    }

    /// Appends an event to the immutable forensic ledger.
    pub async fn log_system_events(&self, event: &SystemEvent) {
        let timestamp = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_millis() as i64;

        let (e_type, agent_id, meta) = match event {
            SystemEvent::ThoughtCommitted {
                agent_id,
                tx_id,
                namespace,
                text,
            } => {
                // Sanitize the text to prevent JSON parsing panics in the meta column
                let safe_text = text
                    .replace("\"", "\\\"")
                    .chars()
                    .take(100)
                    .collect::<String>();
                let m = format!(
                    "{{\"tx_id\": \"{}\", \"namespace\": \"{}\", \"text_preview\": \"{}...\"}}",
                    tx_id, namespace, safe_text
                );

                ("ThoughtCommited", agent_id.clone(), m)
            }

            SystemEvent::AegisInterdiction {
                agent_id,
                attempted_path,
                rule_broken,
                payload,
            } => {
                let m = format!(
                    "{{\"path\": \"{}\", \"rule\": \"{}\", \"payload\": \"{}\"}}",
                    attempted_path, rule_broken, payload
                );
                ("AegisInterdiction", agent_id.clone(), m)
            }

            SystemEvent::CompactionTriggered { archived_count, .. } => (
                "CompactionTriggered",
                "SYSTEM".to_string(),
                format!(" {{ \"archived\": {} }} ", archived_count),
            ),

            SystemEvent::SystemBoot { message } => (
                "SystemBoot",
                "SYSTEM".to_string(),
                format!(" {{ \"message\": \"{}\" }} ", message),
            ),

            SystemEvent::PluginLoaded { plugin_name } => (
                "PluginLoaded",
                "SYSTEM".to_string(),
                format!(" {{ \"plugin_name\": \"{}\" }} ", plugin_name),
            ),

            SystemEvent::SecurityBreach {
                agent_id,
                reason,
                culprit_text,
            } => {
                let m = format!(
                    "{{ \"culprit\": \"{}\", \"reason\": \"{}\"}}",
                    culprit_text, reason
                );

                ("SecurityBreach", agent_id.to_string(), m)
            }

            SystemEvent::LicenseUpdated { .. } => (
                "LicenseUpdated",
                "SYSTEM".to_string(),
                format!(" {{ \"message\": \"{}\" }} ", "License Key was updated"),
            ),

            SystemEvent::MarkleBatchCrystallized { batch } => {
                let m = format!(
                    "{{\"batch_id\": {}, \"namespace\": \"{}\", \"merkle_root\": \"{}\", \"leaves_count\": {}}}",
                    batch.batch_id,
                    batch.namespace,
                    hex::encode(batch.markle_root),
                    batch.leaves.len()
                );
                ("MarkleBatchCrystallized", "SYSTEM".to_string(), m)
            }

            _ => ("default", "default".to_string(), "default".to_string()),
        };

        let time_arr = Arc::new(Int64Array::from(vec![timestamp]));
        let type_arr = Arc::new(StringArray::from(vec![e_type]));
        let agent_arr = Arc::new(StringArray::from(vec![agent_id]));
        let meta_arr = Arc::new(StringArray::from(vec![meta]));

        let batch = RecordBatch::try_new(
            self.audit_schema(),
            vec![time_arr, type_arr, agent_arr, meta_arr],
        );
        let batches = RecordBatchIterator::new(vec![batch], self.audit_schema());

        let table_name = "system_audit_vault";
        let table_names = self.db.table_names().execute().await.unwrap();

        if table_names.contains(&table_name.to_string()) {
            let table = self.db.open_table(table_name).execute().await.unwrap();
            table.add(batches).execute().await.unwrap();
        } else {
            self.db
                .create_table(table_name, batches)
                .execute()
                .await
                .unwrap();
        }
    }

    /// Generate the Enterprise Compliance Report for a specific Agent
    pub async fn generate_compliance_report(
        &self,
        agent_hex: &str,
    ) -> Result<Vec<String>, anyhow::Error> {
        let table = self.db.open_table("system_audit_vault").execute().await?;

        let mut stream = table
            .query()
            .only_if(format!("agent_id = '{}'", agent_hex))
            .execute()
            .await?;

        let mut report = Vec::new();
        while let Some(batch_res) = stream.next().await {
            let batch = batch_res?;
            let time_col = batch
                .column_by_name("timestamp")
                .unwrap()
                .as_any()
                .downcast_ref::<Int64Array>()
                .expect("FATAL: timestamp column isn't Int64Array");
            let event_col = batch
                .column_by_name("event_type")
                .unwrap()
                .as_any()
                .downcast_ref::<StringArray>()
                .expect("FATAL: event-type column isn't StringArray");
            let agent_col = batch
                .column_by_name("agent_id")
                .unwrap()
                .as_any()
                .downcast_ref::<StringArray>()
                .expect("FATAL: agent_id column isn't StringArray");
            let meta_col = batch
                .column_by_name("metadata")
                .unwrap()
                .as_any()
                .downcast_ref::<StringArray>()
                .expect("FATAL: metadata column isn't StringArray");

            for i in 0..time_col.len() {
                report.push(format!(
                    "[{}] {}: Agent: {} - Details: {}",
                    time_col.value(i),
                    event_col.value(i),
                    agent_col.value(i),
                    meta_col.value(i)
                ));
            }
        }
        Ok(report)
    }

    /// Takes a batch of OpLogs and their corresponding calculated vectors
    /// and performs Row-to-Columnar transformation before saving.
    pub async fn archive_batch(&self, logs: &[OpLog], vectors: &[Vec<f32>]) {
        if logs.is_empty() || logs.len() != vectors.len() {
            return;
        }

        // 1. Columnar transformation (Tearing struct apart)
        let tx_ids: Vec<String> = logs
            .iter()
            .map(|l| format!("{:032x}", l.state.transaction_id))
            .collect();
        let agent_ids: Vec<String> = logs.iter().map(|l| hex::encode(l.agent_id)).collect();
        let statuses: Vec<String> = logs
            .iter()
            .map(|l| format!("{:?}", l.state.status))
            .collect();
        let namespaces: Vec<String> = logs.iter().map(|l| l.state.namespace.clone()).collect();

        let seeds: Vec<String> = logs
            .iter()
            .map(|l| serde_json::to_string(&l.entropy_seeds).unwrap_or_else(|_| "[]".to_string()))
            .collect();
        let http_responses: Vec<String> = logs
            .iter()
            .map(|l| {
                serde_json::to_string(&l.network_responses).unwrap_or_else(|_| "[]".to_string())
            })
            .collect();
        let timestmaps: Vec<i64> = logs.iter().map(|l| l.state.timestamp as i64).collect();
        let payloads: Vec<&[u8]> = logs.iter().map(|l| l.delta.as_slice()).collect();
        let texts: Vec<String> = logs.iter().map(|l| l.state.text.clone()).collect();

        // 2. Build the Zero-Copy Arrow Arrays
        let tx_id_array = Arc::new(StringArray::from(tx_ids));
        let agent_id_array = Arc::new(StringArray::from(agent_ids));
        let payload_array = Arc::new(BinaryArray::from(payloads));
        let timestamp_array = Arc::new(Int64Array::from(timestmaps));
        let status_array = Arc::new(StringArray::from(statuses));
        let text_array = Arc::new(StringArray::from(texts));
        let seed_array = Arc::new(StringArray::from(seeds));
        let namespace_array = Arc::new(StringArray::from(namespaces));
        let http_res_array = Arc::new(StringArray::from(http_responses));

        let vector_array = Arc::new(
            FixedSizeListArray::from_iter_primitive::<Float32Type, _, _>(
                vectors
                    .iter()
                    .map(|v| Some(v.iter().map(|&f| Some(f)).collect::<Vec<_>>())),
                self.dims,
            ),
        );

        // 3. Assemble the RecordBatch
        let batch = RecordBatch::try_new(
            self.schema(),
            vec![
                tx_id_array as Arc<dyn arrow_array::Array>,
                agent_id_array as Arc<dyn arrow_array::Array>,
                namespace_array as Arc<dyn arrow_array::Array>,
                timestamp_array as Arc<dyn arrow_array::Array>,
                status_array as Arc<dyn arrow_array::Array>,
                text_array as Arc<dyn arrow_array::Array>,
                seed_array as Arc<dyn arrow_array::Array>,
                http_res_array as Arc<dyn arrow_array::Array>,
                payload_array as Arc<dyn arrow_array::Array>,
                vector_array as Arc<dyn arrow_array::Array>,
            ],
        )
        .expect("Failed to build Arrow RecordBatch");

        // 4. Commit to the DB
        let result: std::result::Result<RecordBatch, arrow_schema::ArrowError> =
            std::result::Result::Ok(batch);
        let batches = RecordBatchIterator::new(vec![result], self.schema());

        let table_names = self.db.table_names().execute().await.unwrap();
        if table_names.contains(&self.history_table) {
            let table = self
                .db
                .open_table(&self.history_table)
                .execute()
                .await
                .unwrap();

            table.add(batches).execute().await.unwrap()
        } else {
            self.db
                .create_table(&self.history_table, batches)
                .execute()
                .await
                .unwrap();
        }
    }

    /// Return (max_tx_id, total_vector_count)
    pub async fn get_vault_metrics(&self) -> Result<(u128, u64), anyhow::Error> {
        let table_res = self.db.open_table(&self.history_table).execute().await;

        let table = match table_res {
            Ok(t) => t,
            Err(_) => return Ok((0, 0)),
        };

        let total_rows = table.count_rows(None).await? as u64;

        if total_rows == 0 {
            return Ok((0, 0));
        }

        // Bypass SQL. Stream the raw Apache Arrow batches and use SIMD max aggregation
        let mut stream = table.query().execute().await?;
        let mut max_tx: u128 = 0;

        if let Some(batch_result) = stream.next().await {
            let batch = batch_result?;
            let tx_col = batch
                .column_by_name("tx_id")
                .unwrap()
                .as_any()
                .downcast_ref::<arrow_array::StringArray>()
                .unwrap();

            for i in 0..tx_col.len() {
                let tx_str = tx_col.value(i);
                let tx = u128::from_str_radix(tx_str, 16).unwrap_or(0);
                if tx > max_tx {
                    max_tx = tx;
                }
            }
        }

        Ok((max_tx, total_rows))
    }

    // REAL RAG: Seaches semantic history using methematical vector proximity
    pub async fn search_memory(
        &self,
        query: &str,
        namespace: &str,
        limit: usize,
    ) -> Result<Vec<String>, anyhow::Error> {
        // 1. Convert English query to mathematical vector

        // Lock the embedder for the exact ms it takes to embed
        let query_vector = self.embedder.embed(query).await?;

        // 2. Open the table
        let table = self.db.open_table(&self.history_table).execute().await?;

        let filter_clause = if namespace.ends_with("*") {
            let prefix = namespace.trim_end_matches("*");
            format!("namespace >= '{}' AND namespace < '{}~'", prefix, prefix)
        } else {
            format!("namespace = '{}'", namespace)
        };

        // 3. Execute High-Speed vector search (IVF-PQ Algorithm)
        let mut stream = table
            .query()
            .nearest_to(query_vector)?
            .only_if(filter_clause)
            .limit(limit)
            .execute()
            .await?;

        let mut results = Vec::new();

        while let Some(batch_result) = stream.next().await {
            let batch = batch_result?;

            // Downcast Specific columns directly from Apache Arrow Memory
            let text_col = batch
                .column_by_name("text")
                .unwrap()
                .as_any()
                .downcast_ref::<StringArray>()
                .unwrap();
            let agent_id_col = batch
                .column_by_name("agent_id")
                .unwrap()
                .as_any()
                .downcast_ref::<StringArray>()
                .unwrap();
            let timestamp_col = batch
                .column_by_name("timestamp")
                .unwrap()
                .as_any()
                .downcast_ref::<Int64Array>()
                .unwrap();
            let status_col = batch
                .column_by_name("status")
                .unwrap()
                .as_any()
                .downcast_ref::<StringArray>()
                .unwrap();

            for i in 0..text_col.len() {
                // Construct a hyper rich string for the LLM's context window
                let memory_str = format!(
                    "[Time: {}] Agent: '{}' ({}) noted: {}",
                    timestamp_col.value(i),
                    agent_id_col.value(i),
                    status_col.value(i),
                    text_col.value(i)
                );

                results.push(memory_str);
            }
        }
        Ok(results)
    }

    /// Saves the 2MB-5MB active memory snapshot to Cold storage.
    pub async fn save_snapshot(
        &self,
        tx_id: u128,
        timestamp: i64,
        agent_hex: &str,
        memory_blob: Vec<u8>,
    ) {
        let tx_array = Arc::new(StringArray::from(vec![format!("{:032x}", tx_id)]));
        let time_array = Arc::new(Int64Array::from(vec![timestamp]));
        let agent_array = Arc::new(StringArray::from(vec![agent_hex.to_string()]));
        let blob_array = Arc::new(BinaryArray::from(vec![memory_blob.as_slice()]));

        let batch = RecordBatch::try_new(
            self.snapshot_schema(),
            vec![tx_array, time_array, agent_array, blob_array],
        );

        // THE FIX: Wrap the batch in a RecordBatchIterator with the correct schema
        let batches = RecordBatchIterator::new(vec![batch], self.snapshot_schema());

        let table_names = self.db.table_names().execute().await.unwrap();
        if table_names.contains(&self.snapshot_table) {
            let table = self
                .db
                .open_table(&self.snapshot_table)
                .execute()
                .await
                .unwrap();
            table.add(batches).execute().await.unwrap();
        } else {
            self.db
                .create_table(&self.snapshot_table, batches)
                .execute()
                .await
                .unwrap();
        }
    }

    /// Fetches the closest snapshot BEFORE or EQUAL TO the txID
    pub async fn fetch_closest_snapshot(
        &self,
        agent_hex: &str,
        target_tx_id: u128,
    ) -> Result<(u128, u64, Vec<u8>), anyhow::Error> {
        let table = self.db.open_table(&self.snapshot_table).execute().await?;

        // SQL-Style Filter: Find the highest TxID for this agent that's <= target.
        let mut stream = table
            .query()
            .only_if(format!(
                "agent_id = '{}' AND tx_id <= '{}' ",
                agent_hex,
                format!("{:032x}", target_tx_id)
            ))
            .limit(1)
            .execute()
            .await?;

        if let Some(batch_result) = stream.next().await {
            let batch = batch_result?;
            let tx_col = batch
                .column_by_name("tx_id")
                .unwrap()
                .as_any()
                .downcast_ref::<StringArray>()
                .unwrap();

            let time_col = batch
                .column_by_name("timestamp")
                .unwrap()
                .as_any()
                .downcast_ref::<Int64Array>()
                .unwrap();

            let blob_col = batch
                .column_by_name("memory_blob")
                .unwrap()
                .as_any()
                .downcast_ref::<BinaryArray>()
                .unwrap();

            return Ok((
                u128::from_str_radix(tx_col.value(0), 16).unwrap_or(0),
                time_col.value(0) as u64,
                blob_col.value(0).to_vec(),
            ));
        }

        Err(anyhow::anyhow!(
            "No snapshot found for agent: {}",
            agent_hex
        ))
    }

    pub async fn get_total_vector_count(&self) -> Result<usize, anyhow::Error> {
        let table = self.db.open_table(&self.history_table).execute().await?;
        Ok(table.count_rows(None).await?)
    }

    /// Executes a SQL Aggregation over the Apache Arrow Memory Layout
    pub async fn get_densest_namespace(&self) -> Result<String, anyhow::Error> {
        let table_res = self.db.open_table(&self.history_table).execute().await;

        let table = match table_res {
            Ok(t) => t,
            Err(_) => return Ok("Empty (0%)".to_string()),
        };

        let total_rows = table.count_rows(None).await? as f64;
        if total_rows == 0.0 {
            return Ok("Empty (0%)".to_string());
        }

        // Stream the raw Arrow columns and aggregate in a Rust Hashmap
        let mut stream = table.query().execute().await?;
        let mut freq_map = HashMap::new();

        while let Some(batch_result) = stream.next().await {
            let batch = batch_result?;
            let ns_col = batch
                .column_by_name("namespace")
                .unwrap()
                .as_any()
                .downcast_ref::<StringArray>()
                .unwrap();

            for i in 0..ns_col.len() {
                let ns = ns_col.value(i);
                *freq_map.entry(ns.to_string()).or_insert(0_u64) += 1;
            }
        }

        // Find the top namespace
        let mut top_ns = String::from("UNKNOWN");
        let mut top_count = 0;

        for (ns, count) in freq_map {
            if count > top_count {
                top_count = count;
                top_ns = ns;
            }
        }

        let percent = (top_count as f64 / total_rows) * 100.0;
        Ok(format!("{} ({:.1})%", top_ns, percent))
    }

    /// Computes the exact size of the LanceDB directory on Disk
    pub async fn get_index_size_mb(&self) -> f64 {
        let table_dir = format!("{}/{}.lance", &self.storage_path, &self.history_table);

        if !Path::new(&table_dir).exists() {
            return 0.0;
        }

        match fs_extra::dir::get_size(&table_dir) {
            Ok(bytes) => bytes as f64 / (1024.0 * 1024.0),
            Err(e) => {
                eprintln!(
                    "[VAULT WARNING] Failed to calculate directory size for {}: {}",
                    table_dir, e
                );
                0.0
            }
        }
    }

    pub async fn fetch_historical_timeline(
        &self,
        agent_hex: &str,
    ) -> Result<Vec<TimelineNode>, anyhow::Error> {
        let table = self.db.open_table(&self.history_table).execute().await?;

        let mut stream = table
            .query()
            .only_if(format!("agent_id = '{}'", agent_hex))
            .execute()
            .await?;

        let mut nodes = Vec::new();

        while let Some(batch_result) = stream.next().await {
            let batch = batch_result?;
            let text_col = batch
                .column_by_name("text")
                .unwrap()
                .as_any()
                .downcast_ref::<StringArray>()
                .unwrap();
            let tx_col = batch
                .column_by_name("tx_id")
                .unwrap()
                .as_any()
                .downcast_ref::<StringArray>()
                .unwrap();
            let status_col = batch
                .column_by_name("status")
                .unwrap()
                .as_any()
                .downcast_ref::<StringArray>()
                .unwrap();
            let timestamp_col = batch
                .column_by_name("timestamp")
                .unwrap()
                .as_any()
                .downcast_ref::<Int64Array>()
                .unwrap();

            for i in 0..tx_col.len() {
                nodes.push(TimelineNode {
                    tx_id: u128::from_str_radix(&tx_col.value(i).to_string().as_str(), 16)
                        .unwrap_or(0),
                    timestamp: timestamp_col.value(i).to_string(),
                    agent_status: status_col.value(i).to_string(),
                    payload_preview: text_col.value(i).to_string(),
                });
            }
        }

        Ok(nodes)
    }
}
