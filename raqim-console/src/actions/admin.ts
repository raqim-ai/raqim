'use server';

import {
  getDashboardCards,
  getClusterTopology,
  liftAegisQuarantine,
  triggerTimeTravel,
  getAgentTimeline,
  getClusterInfo,
  mintCaCertificate,
  toggleIngress,
  getClusterEnclaves,
} from '../lib/api';

import type {
  DashboardCardsData,
  ClusterShard,
  ForkConfigPayload,
  TimelineNode,
  ClusterInfoData,
  ClusterEnclave,
} from '../lib/api';

/**
 * 1. Fetch Dashboard Metrics
 */
export async function fetchDashboardCards(): Promise<DashboardCardsData> {
  const res = await getDashboardCards();
  if (res.success && res.data) {
    return res.data;
  }
  return {
    global_transactions: 0,
    active_agents: 0,
    vault_capacity: 0,
  };
}

/**
 * 2. Fetch Active CRDT Shards / Cluster Topology
 */
export async function fetchTopology(): Promise<ClusterShard[]> {
  const res = await getClusterTopology();
  if (res.success && res.data) {
    return res.data;
  }
  return [];
}

/**
 * 3. Lift Quarantine / Resurrect Agent
 */
export async function liftQuarantine(
  agentId: string,
  systemPromptOverride?: string
): Promise<{ success: boolean; error?: string }> {
  const res = await liftAegisQuarantine({
    agent_hex: agentId,
    system_prompt_override: systemPromptOverride,
  });
  if (res.success) {
    return { success: true };
  }
  return { success: false, error: res.error || 'Failed to lift quarantine' };
}

/**
 * 4. Trigger Reality Fork / Time Machine Endpoint (v1/admin/time_travel)
 */
export async function triggerRealityFork(
  agentId: string,
  txId: number,
  forkConfig?: ForkConfigPayload
): Promise<{ success: boolean; error?: string }> {
  const res = await triggerTimeTravel({
    agent_hex: agentId,
    target_tx_id: txId,
    fork_config: forkConfig,
  });
  if (res.success) {
    return { success: true };
  }
  return { success: false, error: res.error || 'Failed to trigger reality fork' };
}

/**
 * 5. Fetch Agent Timeline Nodes
 */
export async function fetchAgentTimeline(agentHex: string): Promise<TimelineNode[]> {
  const res = await getAgentTimeline(agentHex);
  if (res.success && res.data) {
    return res.data;
  }
  return [];
}

/**
 * 6. Execute Time Travel Action
 */
export async function executeTimeTravel({
  agent_hex,
  target_tx_id,
  fork_config,
}: {
  agent_hex: string;
  target_tx_id: number;
  fork_config: any;
}): Promise<{ success: boolean; error?: string }> {
  const res = await triggerTimeTravel({
    agent_hex,
    target_tx_id,
    fork_config,
  });
  if (res.success) {
    return { success: true };
  }
  return { success: false, error: res.error || 'Failed to execute time travel' };
}

/**
 * 7. Fetch Cluster Diagnostics
 */
export async function fetchClusterDiagnostics(): Promise<ClusterInfoData | null> {
  const res = await getClusterInfo();
  if (res.success && res.data) {
    return res.data;
  }
  return null;
}

/**
 * 8. Mint Capability Certificate
 */
export async function mintCertificate(
  agentHex: string,
  group: string
): Promise<{ success: boolean; certHex?: string; error?: string }> {
  const res = await mintCaCertificate({ agent_hex: agentHex, group });
  if (res.success && res.data) {
    return { success: true, certHex: res.data };
  }
  return { success: false, error: res.error || 'Failed to mint certificate' };
}

/**
 * 9. Toggle Ingress Action
 */
export async function toggleIngressAction(): Promise<{
  success: boolean;
  is_ingress_paused?: boolean;
  error?: string;
}> {
  const res = await toggleIngress();
  if (res.success && res.data) {
    return { success: true, is_ingress_paused: res.data.is_ingress_paused };
  }
  return { success: false, error: res.error || 'Failed to toggle ingress' };
}

/**
 * 10. Fetch Cluster Enclaves
 */
export async function fetchClusterEnclaves(): Promise<ClusterEnclave[]> {
  const res = await getClusterEnclaves();
  if (res.success && res.data) {
    return res.data;
  }
  return [];
}
