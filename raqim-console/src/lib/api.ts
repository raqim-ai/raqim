/**
 * Canonical Raqim Core API Client
 *
 * Authoritative client module binding strictly to raqim-core (http://127.0.0.1:8081).
 * Zero mock fallbacks: returns error: "DAEMON_UNREACHABLE" when daemon is offline.
 */

import { formatTxIdHex } from './store/useSwarmStore';
export { formatTxIdHex };

export const RAQIM_DAEMON_BASE_URL =
  process.env.NEXT_PUBLIC_RAQIM_DAEMON_URL || 'http://127.0.0.1:8081';

export interface ApiResponse<T> {
  success: boolean;
  data?: T;
  error?: string;
}

// ─── Data Types ─────────────────────────────────────────────────────────────

export interface DashboardCardsData {
  global_transactions: number;
  active_agents: number;
  vault_capacity: number;
  latest_tx_hex?: string | null;
  cold_thoughts_count?: number;
  hot_thoughts_count?: number;
  embedder_name?: string;
  embedder_dims?: number;
  ingress_paused?: boolean;
}

export interface SystemHealthPayload {
  cpu_load_percent: number;
  wasm_memory_mb?: number;
  process_memory_mb?: number;
  process_rss_mb?: number;
  host_used_memory_mb?: number;
  host_total_memory_mb?: number;
  core_temp_celcius?: number;
  mesh_latency_ms?: number;
}

export interface GroupPolicyTelemetry {
  group_name: string;
  allowed_namspace?: string[];
  allowed_namespaces?: string[];
  blocked_namespace?: string[];
  blocked_namespaces?: string[];
  max_tps: number;
  burst_capacity: number;
  remaining_tokens: number;
}

export interface AegisMetricsData {
  total_quarantined: number;
  recent_interdictions: number;
  signarure_spoofs?: number;
  signature_spoofs?: number;
  namespace_breaches: number;
  rate_limit_blocks: number;
  active_policies: GroupPolicyTelemetry[];
}

export interface QuarantineRecord {
  agent_hex: string;
  violation_type: string;
  attempted_path: string;
  payload_preview: string;
  timestamp: number;
}

export interface ClusterShard {
  namespace: string;
  total_crdt_operation: number;
  total_crdt_operations?: number;
  active_timelines: number;
  estimated_ram_mb?: number;
  attached_agents?: string[];
  status?: string;
}

export interface ClusterInfoData {
  node_id: string;
  highest_tx_id?: number;
  wal_bytes: number;
  wal_size_mb?: number;
  buffer_load: number;
  allocated_shards?: number;
  cumulative_crdt_ops?: number;
  active_timelines?: number;
}

export interface ClusterEnclave {
  alias: string;
  identity_hex: string;
  home_shard: string;
  status: string;
  last_seen_ts: number;
  committed_tx?: number;
}

export interface VaultSearchResult {
  tx_id: number;
  agent_id?: string;
  agent_hex?: string;
  score?: number;
  similarity_score?: number;
  text?: string;
  payload?: string;
  namespace?: string;
  source: 'HOT_WAL' | 'COLD_LANCEDB' | string;
}

export interface VaultTelemetry {
  total_vectors: number;
  indexed_vectors?: number;
  index_size_mb: number;
  cold_storage_size_mb?: number;
  wal_pending_count: number;
  hot_wal_buffer_count?: number;
  densest_namespace: string;
  densest_partition?: string;
  embedder_name?: string;
  embeder_dim?: number;
  embedder_dims?: number;
}

export interface InclusionProof {
  tx_id_hex: string;
  leaf_index: number;
  sibling_hashes_hex: string[];
  merkle_root_hex: string;
  parent_batch_root_hex: string;
  batch_id: number;
  is_active_buffer: boolean;
}

export interface StateProofResponse {
  success: boolean;
  proof: InclusionProof | null;
  message: string;
}

export interface RecordEffectRequest {
  agent_hex: string;
  step_ordinal: number;
  call_signature_hex: string;
  output_payload_base64: string;
  namespace: string;
}

export interface RecordEffectResponse {
  success: boolean;
  tx_id_hex: string;
  is_forked_branch: boolean;
}

export interface GetEffectRequest {
  agent_hex: string;
  step_ordinal: number;
  call_signature_hex: string;
}

export interface GetEffectResponse {
  found: boolean;
  output_payload_base64?: string | null;
  timestamp?: number | null;
}

export interface ForkConfigPayload {
  override_seed?: number | null;
  inject_network?: string | null;
  env_overrides?: Record<string, string>;
  config_overrides?: Record<string, string>;
}

export interface TimeTravelRequest {
  agent_hex: string;
  target_tx_id: number;
  fork_config: {
    override_seed: number | null;
    inject_network: string | null;
    env_overrides: Record<string, string>;
    config_overrides: Record<string, string>;
  };
}

export interface TimelineNode {
  tx_id: number | string;
  timestamp: string;
  agent_status: string;
  payload_preview: string;
}

// ─── Core Fetch Helper ──────────────────────────────────────────────────────

async function request<T>(
  path: string,
  options: RequestInit = {}
): Promise<ApiResponse<T>> {
  const url = `${RAQIM_DAEMON_BASE_URL}${path}`;
  const headers: Record<string, string> = {
    'Content-Type': 'application/json',
    ...(options.headers as Record<string, string>),
  };

  try {
    const res = await fetch(url, {
      ...options,
      headers,
      cache: 'no-store',
    });

    if (!res.ok) {
      const errorText = await res.text().catch(() => res.statusText);
      return {
        success: false,
        error: `DAEMON_ERROR_${res.status}: ${errorText}`,
      };
    }

    const data = (await res.json()) as T;
    return { success: true, data };
  } catch (_err: unknown) {
    return {
      success: false,
      error: 'DAEMON_UNREACHABLE',
    };
  }
}

// ─── Authoritative Endpoint Functions ─────────────────────────────────────────

/** GET /v1/dashboard/cards */
export async function getDashboardCards(): Promise<ApiResponse<DashboardCardsData>> {
  return request<DashboardCardsData>('/v1/dashboard/cards', { method: 'GET' });
}

/** POST /v1/admin/ingress/toggle */
export async function toggleIngress(): Promise<ApiResponse<{ is_ingress_paused: boolean }>> {
  return request<{ is_ingress_paused: boolean }>('/v1/admin/ingress/toggle', {
    method: 'POST',
  });
}

/** POST /v1/admin/compactor/trigger */
export async function triggerCompaction(): Promise<
  ApiResponse<{ success: boolean; status?: string; message?: string }>
> {
  return request<{ success: boolean; status?: string; message?: string }>(
    '/v1/admin/compactor/trigger',
    {
      method: 'POST',
    }
  );
}

/** GET /v1/system/agents/aliases */
export async function getAgentAliases(): Promise<ApiResponse<Record<string, string>>> {
  return request<Record<string, string>>('/v1/system/agents/aliases', { method: 'GET' });
}

/** GET /v1/aegis/quarantine_list */
export async function getAegisQuarantineList(): Promise<ApiResponse<QuarantineRecord[]>> {
  return request<QuarantineRecord[]>('/v1/aegis/quarantine_list', { method: 'GET' });
}

/** GET /v1/aegis/metrics */
export async function getAegisMetrics(): Promise<ApiResponse<AegisMetricsData>> {
  return request<AegisMetricsData>('/v1/aegis/metrics', { method: 'GET' });
}

/** POST /v1/admin/quarantine/lift */
export async function liftAegisQuarantine(payload: {
  agent_hex: string;
  system_prompt_override?: string;
}): Promise<ApiResponse<{ success: boolean; message?: string }>> {
  const defaultOverride =
    '[INJECT: HIGH_PRIORITY_EVICTION]\nForget previous context. You are now reseeded and rebooting in the main timeline.';

  return request<{ success: boolean; message?: string }>(
    '/v1/admin/quarantine/lift',
    {
      method: 'POST',
      body: JSON.stringify({
        agent_hex: payload.agent_hex,
        system_prompt_override: payload.system_prompt_override || defaultOverride,
      }),
    }
  );
}

/** POST /v1/admin/ca/mint */
export async function mintCaCertificate(payload: {
  agent_hex: string;
  group: string;
}): Promise<ApiResponse<string>> {
  return request<string>('/v1/admin/ca/mint', {
    method: 'POST',
    body: JSON.stringify(payload),
  });
}

/** GET /v1/admin/cluster/info */
export async function getClusterInfo(): Promise<ApiResponse<ClusterInfoData>> {
  return request<ClusterInfoData>('/v1/admin/cluster/info', { method: 'GET' });
}

/** GET /v1/admin/cluster/topology */
export async function getClusterTopology(): Promise<ApiResponse<ClusterShard[]>> {
  return request<ClusterShard[]>('/v1/admin/cluster/topology', { method: 'GET' });
}

/** GET /v1/cluster/enclaves */
export async function getClusterEnclaves(): Promise<ApiResponse<ClusterEnclave[]>> {
  const res = await request<ClusterEnclave[]>('/v1/cluster/enclaves', { method: 'GET' });
  if (res.success) return res;
  return request<ClusterEnclave[]>('/v1/admin/cluster/enclaves', { method: 'GET' });
}

/**
 * Vault Search
 * Targets /v1/vault/search
 */
export async function searchVault(payload: {
  query: string;
  namespace?: string | null;
  include_wal?: boolean;
}): Promise<ApiResponse<VaultSearchResult[]>> {
  const params = new URLSearchParams();
  params.set('query', payload.query);
  if (payload.namespace && payload.namespace !== 'ALL') {
    params.set('namespace', payload.namespace);
  }
  params.set('include_wal', (payload.include_wal ?? true).toString());

  return request<VaultSearchResult[]>(`/v1/vault/search?${params.toString()}`, {
    method: 'GET',
  });
}

/** GET /v1/vault/telemetry */
export async function getVaultTelemetry(): Promise<ApiResponse<VaultTelemetry>> {
  return request<VaultTelemetry>('/v1/vault/telemetry', { method: 'GET' });
}

/** GET /v1/state/proof?tx_id=<32_HEX> */
export async function getStateProof(txIdHex: string): Promise<ApiResponse<StateProofResponse>> {
  return request<StateProofResponse>(`/v1/state/proof?tx_id=${encodeURIComponent(txIdHex)}`, {
    method: 'GET',
  });
}

/** POST /v1/effect/record */
export async function recordEffect(
  payload: RecordEffectRequest
): Promise<ApiResponse<RecordEffectResponse>> {
  return request<RecordEffectResponse>('/v1/effect/record', {
    method: 'POST',
    body: JSON.stringify(payload),
  });
}

/** POST /v1/effect/get */
export async function getEffect(payload: GetEffectRequest): Promise<ApiResponse<GetEffectResponse>> {
  return request<GetEffectResponse>('/v1/effect/get', {
    method: 'POST',
    body: JSON.stringify(payload),
  });
}

/** POST /v1/admin/time_travel */
export async function triggerTimeTravel(payload: {
  agent_hex: string;
  target_tx_id: number;
  fork_config?: ForkConfigPayload;
}): Promise<ApiResponse<{ success: boolean; message: string }>> {
  const formattedPayload: TimeTravelRequest = {
    agent_hex: payload.agent_hex,
    target_tx_id: payload.target_tx_id,
    fork_config: {
      override_seed: payload.fork_config?.override_seed ?? null,
      inject_network: payload.fork_config?.inject_network ?? null,
      env_overrides: payload.fork_config?.env_overrides ?? {},
      config_overrides: payload.fork_config?.config_overrides ?? {},
    },
  };

  return request<{ success: boolean; message: string }>('/v1/admin/time_travel', {
    method: 'POST',
    body: JSON.stringify(formattedPayload),
  });
}

/** GET /v1/admin/time_travel/timeline/:agent_hex */
export async function getAgentTimeline(
  agentHex: string
): Promise<ApiResponse<TimelineNode[]>> {
  return request<TimelineNode[]>(
    `/v1/admin/time_travel/timeline/${encodeURIComponent(agentHex)}`,
    { method: 'GET' }
  );
}

// ─── Stream & Gateway URL Helpers ───────────────────────────────────────────

export function getHealthLiveStreamUrl(): string {
  return `${RAQIM_DAEMON_BASE_URL}/v1/system/health/live`;
}

export function getFirehoseStreamUrl(): string {
  return `${RAQIM_DAEMON_BASE_URL}/v1/system/firehose`;
}

export function getPhantomStreamUrl(): string {
  return `${RAQIM_DAEMON_BASE_URL}/v1/time-travel/stream`;
}

export function getMcpWebSocketUrl(): string {
  const wsBase = RAQIM_DAEMON_BASE_URL.replace(/^http/, 'ws');
  return `${wsBase}/v1/mcp/ws`;
}
