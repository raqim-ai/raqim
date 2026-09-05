'use server';

import {
  searchVault,
  getVaultTelemetry as getCanonicalVaultTelemetry,
  getStateProof,
  triggerCompaction,
} from '../lib/api';

import type {
  VaultSearchResult,
  VaultTelemetry,
  StateProofResponse,
} from '../lib/api';

/**
 * Server Action to run unified semantic and lexical search.
 */
export async function executeUnifiedSearch({
  query,
  namespace,
  include_wal,
}: {
  query: string;
  namespace: string;
  include_wal: boolean;
}): Promise<VaultSearchResult[]> {
  const res = await searchVault({
    query,
    namespace: namespace === 'ALL' ? undefined : namespace,
    include_wal,
  });

  if (res.success && res.data) {
    return res.data;
  }
  return [];
}

export async function fetchVaultSearchResults(
  query: string,
  namespace?: string,
  includeWal: boolean = true
): Promise<VaultSearchResult[]> {
  return executeUnifiedSearch({
    query,
    namespace: namespace || 'ALL',
    include_wal: includeWal,
  });
}

/**
 * Server Action to fetch Vault Index Vitals & Telemetry.
 */
export async function getVaultTelemetry(): Promise<VaultTelemetry> {
  const res = await getCanonicalVaultTelemetry();
  if (res.success && res.data) {
    return res.data;
  }
  return {
    total_vectors: 0,
    index_size_mb: 0,
    wal_pending_count: 0,
    densest_namespace: 'UNKNOWN (0%)',
  };
}

export async function fetchVaultTelemetry(): Promise<VaultTelemetry> {
  return getVaultTelemetry();
}

/**
 * Server Action to query Axon for an inclusion state proof for a given 32-char Hex TxID.
 */
export async function fetchStateProof(txIdHex: string): Promise<StateProofResponse> {
  const res = await getStateProof(txIdHex);
  if (res.success && res.data) {
    return res.data;
  }
  return {
    success: false,
    proof: null,
    message: res.error || 'Transaction ID not found in active memory batch archives.',
  };
}

/**
 * Server Action to trigger manual on-demand WAL compaction into LanceDB.
 * Targets POST /v1/admin/compactor/trigger
 */
export async function triggerCompactionAction(): Promise<{
  success: boolean;
  message?: string;
  error?: string;
}> {
  const res = await triggerCompaction();
  if (res.success) {
    return {
      success: true,
      message:
        (typeof res.data?.message === 'string' && res.data.message) ||
        'WAL segment rotated. 2PC LanceDB assimilation initiated in background.',
    };
  }
  return { success: false, error: res.error || 'Failed to trigger compaction' };
}
