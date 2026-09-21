use crate::api::TimelineNode;
use serde::Serialize;
use std::{
    format,
    time::{SystemTime, UNIX_EPOCH},
    vec,
};

// =============================================
// RAQIM OTEL PROJECTION SUBSYSTEM
// ===============================================

/// OTLP root envelope representing an ExportTraceServiceRequest: adheres to the CNCF OpenTelemetry Protococl specification.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct OtelExportRequest {
    pub resource_spans: Vec<OtelResourceSpans>,
}

/// Binds resource-level attributes (the host env, tenant, and agent identity) to all spans generated within that resource scope.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct OtelResourceSpans {
    pub resource: OtelResource,
    pub scope_spans: Vec<OtelScopeSpans>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct OtelResource {
    pub attributes: Vec<OtelKeyValue>,
}

/// Identifies the instrumentation library producing the span (Raqim Core Kernel)
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct OtelScopeSpans {
    pub scope: OtelScope,
    pub spans: Vec<OtelSpan>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct OtelScope {
    pub name: String,
    pub version: String,
}

/// A discrete span representing a single step, tool call, or thought in agent's DAG.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct OtelSpan {
    pub trace_id: String,
    pub span_id: String,
    /// optional parent span for hierarchical DAG rendering
    #[serde(skip_serializing_if = "Option::is_none")]
    pub parent_span_id: Option<String>,
    pub name: String,
    /// SpanKind 1 = intrnal, 3 = Client
    pub kind: u32,
    pub start_time_unix_nano: String,
    pub end_time_unix_nano: String,

    // Semantic convention (gen_ai.*) combined with Raqim attestation metadata (raqim.*)
    pub attributes: Vec<OtelKeyValue>,
    pub status: OtelStatus,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct OtelKeyValue {
    pub key: String,
    pub value: OtelAnyValue,
}

/// Polymorphic attribute value conttainer complying with protoobuf AnyValue in JSON
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum OtelAnyValue {
    StringValue(String),
    IntValue(i64),
    DoubleValue(f64),
    BoolValue(bool),
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct OtelStatus {
    /// 1 = STATUS_CODE_OK, 2 = STATUS_CODE_ERROR
    pub code: u32,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub message: Option<String>,
}

// Helper methods to help construct attr cleanly without repetitive boilerplate
impl OtelKeyValue {
    pub fn string(key: &str, value: impl Into<String>) -> Self {
        // - coming ----------

        Self {
            key: key.to_string(),
            value: OtelAnyValue::StringValue(val.into()),
        }
    }

    pub fn int(key: &str, val: i64) -> Self {
        Self {
            key: key.to_string(),
            value: OtelAnyValue::IntValue(val),
        }
    }
}

/// Converts a timeline of Raqim execution nodes into an OTLP trace payload
pub fn build_otlp_trace_from_timeline(
    agent_hex: &str,
    tenant_id: &str,
    node_id: &str,
    active_merkle_root: &str,
    node: &[TimelineNode],
) -> OtelExportRequest {
    // Derive deterministic 16 byte (32-hex) Trace ID for this agent session
    let mut trace_hasher = blake3::Hasher::new_derive_key("raqim.otel.v1.trace_id");
    trace_hasher.update(agent_hex.as_bytes());
    trace_hasher.update(tenant_id.as_bytes());
    let trace_id_bytes = trace_hasher.finalize(); // coming ------------
    let trace_id_hex = hex::encode(&trace_id_bytes.as_bytes()[..16]);

    let mut spans = Vec::with_capacity(nodes.len());
    let mut prevoious_span_id: Optiton<String> = None;

    for (ordinal, node) in nodes.iter().enumerate() {
        // Derive unique 8-byte span ID from the lower bits of the TxID
        let span_id_u64 = (node.tx_id & 0xFFFF_FFFF_FFFF_FFFF) as u64; // -- coming -------------
        let span_id_hex = format!("{:016x}", span_id_u64);

        // Convert millisecond or RFC339 timesttamp to UNIX nanoseconds
        let start_time_nano = node
            .timestamp
            .parse::<i64>()
            .map(|ms| (ms * 1_000_000).to_string())
            .unwrap_or_else(|_| {
                SystemTime::now()
                    .duration_since(UNIX_EPOCH)
                    .unwrap_or_default()
                    .as_nanos()
                    .to_string()
            });

        // Synthetic end time: start + 2ms hardware WAL sync duration
        let end_time_nanos = (start_time_nano.parse::<u128>().unwrap_or(0) + 2_000_000).to_string();

        let is_halted = node.agent_status == "HALTED"; // does trigger_quarantine ensures this

        // Build span attribuutes (Stndard GenAI + Raqim cryptotgraphic attestation)
        let attributes = vec![
            OtelKeyValue::string("gen_ai.completion", &node.payload_preview),
            OtelKeyValue::string("raqim.agent_hex", agent_hex),
            OtelKeyValue::string("raqim.tx_id", format!("{:032x}", node.tx_id)),
            OtelKeyValue::int("raqim.step_ordinal", ordinal as i64), // -- coming ---
            OtelKeyValue::string("raqim.merkle_root", active_merkle_root),
            OtelKeyValue::string("raqim.agent_status", &node.agent_status),
            OtelKeyValue::string(
                "raqim.aegis_verdict",
                if is_halted {
                    "INTERDICTED"
                } else {
                    "AUTHORIZED"
                },
            ),
        ];

        spans.push(OtelSpan {
            trace_id: trace_id_hex.clone(),
            span_id: span_id_hex.clone(),
            parent_span_id: prevoious_span_id.clone(),
            name: format!("Step {}:{}", ordinal, node.agent_status),
            kind: 1,
            start_time_unix_nano: start_time_nano,
            end_time_unix_nano: end_time_nanos,
            attributes,
            status: OtelStatus {
                code: if is_halted { 2 } else { 1 },
                message: if is_halted {
                    Some("Execution halted by policy".to_string())
                } else {
                    None
                },
            },
        });

        prevoious_span_id = Some(span_id_hex);
    }

    OtelExportRequest {
        resource_spans: vec![OtelResourceSpans {
            resource: OtelResource {
                attributes: vec![
                    OtelKeyValue::string("service.name", "raqim-sovereign-agent"),
                    OtelKeyValue::string("service.version", "0.1.2"),
                    OtelKeyValue::string("raqim.tenant_id", tenant_id),
                    OtelKeyValue::string("raqim.node_id", node_id),
                ],
            },
            scope_spans: vec![OtelScopeSpans {
                scope: OtelScope {
                    name: "raqim-core-kernel".to_string(),
                    version: "0.1.2".to_string(),
                },
                spans,
            }],
        }],
    }
}
