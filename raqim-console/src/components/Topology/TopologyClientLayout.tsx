'use client';

import React, { useState, useEffect } from 'react';
import { MainLayout } from '../Layout/MainLayout';
import { ReactFlowProvider } from '@xyflow/react';
import { ClusterTelemetryRibbon } from './ClusterTelemetryRibbon';
import { TopologyCanvas } from './TopologyCanvas';
import { ShardDetailDrawer } from './ShardDetailDrawer';
import { AgentProcessTable } from './AgentProcessTable';
import { ClusterShard, ClusterInfoData, ClusterEnclave } from '../../lib/api';
import { fetchTopology, fetchClusterDiagnostics, fetchClusterEnclaves } from '../../actions/admin';
import { useSwarmStore } from '../../lib/store/useSwarmStore';
import { useSwarmStream } from '../../lib/hooks/useSwarmStream';

interface TopologyClientLayoutProps {
  initialTopology: ClusterShard[];
  initialClusterInfo: ClusterInfoData | null;
  initialAliases: Record<string, string>;
  initialEnclaves: ClusterEnclave[];
}

export function TopologyClientLayout({
  initialTopology,
  initialClusterInfo,
  initialAliases,
  initialEnclaves = [],
}: TopologyClientLayoutProps) {
  useSwarmStream();

  const [shards, setShards] = useState<ClusterShard[]>(initialTopology);
  const [clusterInfo, setClusterInfo] = useState<ClusterInfoData | null>(initialClusterInfo);
  const [enclaves, setEnclaves] = useState<ClusterEnclave[]>(initialEnclaves);
  const [selectedShard, setSelectedShard] = useState<ClusterShard | null>(null);

  const setAgentAliases = useSwarmStore((state) => state.setAgentAliases);
  const setActiveTopology = useSwarmStore((state) => state.setActiveTopology);
  const setStoreClusterInfo = useSwarmStore((state) => state.setClusterInfo);
  const agentAliases = useSwarmStore((state) => state.agentAliases);

  useEffect(() => {
    if (initialAliases) setAgentAliases(initialAliases);
    if (initialClusterInfo) setStoreClusterInfo(initialClusterInfo);
    if (initialTopology) setActiveTopology(initialTopology);
  }, [initialAliases, initialClusterInfo, initialTopology, setAgentAliases, setStoreClusterInfo, setActiveTopology]);

  // Periodic polling every 5 seconds for cluster topology updates
  useEffect(() => {
    const syncData = async () => {
      try {
        const [top, diag, enc] = await Promise.all([
          fetchTopology(),
          fetchClusterDiagnostics(),
          fetchClusterEnclaves(),
        ]);
        if (top) {
          setShards(top);
          setActiveTopology(top);
        }
        if (diag) {
          setClusterInfo(diag);
          setStoreClusterInfo(diag);
        }
        if (enc) {
          setEnclaves(enc);
        }
      } catch (_e) {
        // Quiet poll error
      }
    };

    const interval = setInterval(syncData, 5000);
    return () => clearInterval(interval);
  }, [setActiveTopology, setStoreClusterInfo]);

  const totalActiveAgents = enclaves.length > 0 ? enclaves.length : Object.keys(agentAliases).length;

  return (
    <MainLayout title="Swarm Topology // Distributed CRDT Matrix">
      <div className="flex flex-col h-full w-full bg-zinc-950 overflow-hidden p-3 gap-3">
        {/* 1. Cluster Status Ribbon */}
        <ClusterTelemetryRibbon
          clusterInfo={clusterInfo}
          shards={shards}
          totalActiveAgents={totalActiveAgents}
        />

        {/* 2. Interactive Canvas Container */}
        <div className="flex-1 min-h-[420px] bg-zinc-950 border border-zinc-800/80 rounded-sm overflow-hidden relative shadow-xl">
          <ReactFlowProvider>
            <TopologyCanvas
              shards={shards}
              clusterInfo={clusterInfo}
              onSelectShard={(shard) => setSelectedShard(shard)}
              selectedShardNamespace={selectedShard?.namespace || null}
            />
          </ReactFlowProvider>

          {/* Shard Forensic Detail Drawer */}
          {selectedShard && (
            <ShardDetailDrawer
              shard={selectedShard}
              onClose={() => setSelectedShard(null)}
            />
          )}
        </div>

        {/* 3. High-Density Agent Process Matrix */}
        <div className="shrink-0 max-h-60 overflow-hidden">
          <AgentProcessTable enclaves={enclaves} agentAliases={agentAliases} />
        </div>
      </div>
    </MainLayout>
  );
}
