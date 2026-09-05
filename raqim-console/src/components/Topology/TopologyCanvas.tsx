'use client';

import React, { useMemo, useCallback, useEffect } from 'react';
import {
  ReactFlow,
  Background,
  BackgroundVariant,
  Node,
  Edge,
  useReactFlow,
} from '@xyflow/react';
import '@xyflow/react/dist/style.css';

import { KernelNode } from './KernelNode';
import { ShardNode } from './ShardNode';
import { AgentNode } from './AgentNode';
import { A2aBeamEdge } from './A2aBeamEdge';
import { ClusterShard, ClusterInfoData } from '../../lib/api';
import { useSwarmStore } from '../../lib/store/useSwarmStore';
import { ZoomIn, ZoomOut, Maximize, RotateCcw, Lock, Unlock } from 'lucide-react';

const nodeTypes = {
  kernel: KernelNode,
  shard: ShardNode,
  agent: AgentNode,
};

const edgeTypes = {
  a2a: A2aBeamEdge,
};

interface TopologyCanvasProps {
  shards: ClusterShard[];
  clusterInfo: ClusterInfoData | null;
  onSelectShard: (shard: ClusterShard) => void;
  selectedShardNamespace: string | null;
}

export function TopologyCanvas({
  shards,
  clusterInfo,
  onSelectShard,
  selectedShardNamespace,
}: TopologyCanvasProps) {
  const { fitView, zoomIn, zoomOut, setCenter } = useReactFlow();

  const topologyEdges = useSwarmStore((state) => state.topologyEdges);
  const agentAliases = useSwarmStore((state) => state.agentAliases);
  const quarantinedAgents = useSwarmStore((state) => state.quarantinedAgents);
  const thoughts = useSwarmStore((state) => state.thoughts);
  const thoughtOrder = useSwarmStore((state) => state.thoughtOrder);

  const [isLocked, setIsLocked] = React.useState(false);

  // Group active agents by their home shard namespace
  const agentsByNamespace = useMemo(() => {
    const map = new Map<string, Set<string>>();

    for (const hex of Object.keys(agentAliases)) {
      if (!map.has('/default')) map.set('/default', new Set());
      map.get('/default')!.add(hex);
    }

    for (let i = thoughtOrder.length - 1; i >= 0; i--) {
      const t = thoughts[thoughtOrder[i]];
      if (t) {
        const ns = t.intent_path;
        if (!map.has(ns)) map.set(ns, new Set());
        map.get(ns)!.add(t.agent_hex);
      }
    }

    return map;
  }, [agentAliases, thoughts, thoughtOrder]);

  // Compute Multi-Ring Collision-Free Layout
  const { layoutNodes, layoutEdges } = useMemo(() => {
    const nodes: Node[] = [];
    const edges: Edge[] = [];

    // 1. Kernel Node at (0, 0)
    const nodeId = clusterInfo?.node_id || 'RAQIM-CORE-APEX';
    nodes.push({
      id: 'kernel-core',
      type: 'kernel',
      position: { x: -72, y: -72 }, // Centered on origin
      data: {
        nodeId,
        bufferLoad: clusterInfo?.buffer_load ?? 0,
      },
    });

    // 2. Multi-Ring Shard Distribution Parameters
    const ringCapacities = [8, 14, 20, 26, 32];
    const ringRadii = [300, 560, 820, 1080, 1340];

    let currentRing = 0;
    let indexInRing = 0;

    shards.forEach((shard, sIdx) => {
      // Advance to next concentric ring when capacity is reached
      if (indexInRing >= ringCapacities[currentRing] && currentRing < ringCapacities.length - 1) {
        currentRing++;
        indexInRing = 0;
      }

      const totalInRing = Math.min(
        ringCapacities[currentRing],
        shards.length - sIdx + indexInRing
      );
      const ringRadius = ringRadii[currentRing];

      // Alternating angular offset per ring to avoid collinear stacking
      const ringAngleOffset = (currentRing % 2 === 1 ? Math.PI / totalInRing : 0) - Math.PI / 2;
      const angle = (indexInRing / totalInRing) * 2 * Math.PI + ringAngleOffset;

      const shardX = ringRadius * Math.cos(angle) - 128;
      const shardY = ringRadius * Math.sin(angle) - 60;
      const shardNodeId = `shard-${shard.namespace}`;
      const isSelected = selectedShardNamespace === shard.namespace;

      nodes.push({
        id: shardNodeId,
        type: 'shard',
        position: { x: shardX, y: shardY },
        data: {
          namespace: shard.namespace,
          active_timelines: shard.active_timelines,
          total_crdt_operations: shard.total_crdt_operations ?? shard.total_crdt_operation ?? 0,
          isSelected,
          shard,
        },
      });

      // Base edge from Kernel to Shard
      edges.push({
        id: `edge-kernel-${shard.namespace}`,
        source: 'kernel-core',
        target: shardNodeId,
        type: 'default',
        style: { stroke: isSelected ? '#00f3ff' : '#27272a', strokeWidth: isSelected ? 2 : 1 },
      });

      // 3. Orbiting Agents attached to this Shard
      const attachedAgents = Array.from(agentsByNamespace.get(shard.namespace) || []);
      const totalAgents = attachedAgents.length;

      attachedAgents.forEach((agentHex, aIdx) => {
        const agentOffsetAngle =
          angle + (totalAgents > 1 ? (aIdx - (totalAgents - 1) / 2) * 0.35 : 0);
        const agentRadius = ringRadius + 150;
        const agentX = agentRadius * Math.cos(agentOffsetAngle) - 88;
        const agentY = agentRadius * Math.sin(agentOffsetAngle) - 36;
        const agentNodeId = `agent-${agentHex}`;

        const isQuarantined = quarantinedAgents.includes(agentHex);
        const alias = agentAliases[agentHex] || `agent_${agentHex.slice(0, 6)}`;

        if (!nodes.some((n) => n.id === agentNodeId)) {
          nodes.push({
            id: agentNodeId,
            type: 'agent',
            position: { x: agentX, y: agentY },
            data: {
              agent_hex: agentHex,
              alias,
              isQuarantined,
            },
          });

          // Edge from Shard to Agent
          edges.push({
            id: `edge-${shardNodeId}-${agentNodeId}`,
            source: shardNodeId,
            target: agentNodeId,
            type: 'default',
            style: { stroke: '#18181b', strokeWidth: 1 },
          });
        }
      });

      indexInRing++;
    });

    // Merge live dynamic A2A edges from store
    for (const a2aEdge of topologyEdges) {
      if (!edges.some((e) => e.id === a2aEdge.id)) {
        edges.push(a2aEdge);
      }
    }

    return { layoutNodes: nodes, layoutEdges: edges };
  }, [
    shards,
    clusterInfo,
    agentsByNamespace,
    quarantinedAgents,
    agentAliases,
    topologyEdges,
    selectedShardNamespace,
  ]);

  // Handle node selection
  const handleNodeClick = useCallback(
    (_: React.MouseEvent, node: Node) => {
      if (node.type === 'shard' || node.type === 'cluster') {
        const shard = (node.data?.shard as ClusterShard) || {
          namespace: (node.data?.namespace as string) || (node.data?.label as string),
          active_timelines: (node.data?.active_timelines as number) || 0,
          total_crdt_operations: (node.data?.total_crdt_operations as number) || 0,
        };
        onSelectShard(shard);
      }
    },
    [onSelectShard]
  );

  // Initial fit view
  useEffect(() => {
    const timer = setTimeout(() => {
      fitView({ padding: 0.2, duration: 600 });
    }, 150);
    return () => clearTimeout(timer);
  }, [fitView, shards.length]);

  const handleResetView = () => {
    setCenter(0, 0, { zoom: 0.85, duration: 600 });
  };

  return (
    <div className="w-full h-full relative bg-zinc-950 rounded-sm overflow-hidden select-none border border-zinc-800/80">
      <ReactFlow
        nodes={layoutNodes}
        edges={layoutEdges}
        nodeTypes={nodeTypes}
        edgeTypes={edgeTypes}
        onNodeClick={handleNodeClick}
        panOnDrag={!isLocked}
        zoomOnScroll={!isLocked}
        zoomOnPinch={!isLocked}
        nodesDraggable={!isLocked}
        fitView
        minZoom={0.1}
        maxZoom={2.5}
        proOptions={{ hideAttribution: true }}
      >
        <Background
          variant={BackgroundVariant.Dots}
          gap={24}
          size={1}
          color="#27272a"
          className="opacity-60"
        />
      </ReactFlow>

      {/* Floating Canvas Controls Toolbar (Top Right) */}
      <div className="absolute top-3 right-3 z-20 flex items-center gap-1 bg-zinc-900/90 border border-zinc-800 p-1 rounded-xs shadow-xl backdrop-blur-xs font-mono text-xs">
        <button
          onClick={() => zoomIn({ duration: 200 })}
          title="Zoom In"
          className="p-1.5 text-zinc-400 hover:text-white rounded-xs hover:bg-zinc-800 transition-colors"
        >
          <ZoomIn className="w-3.5 h-3.5" />
        </button>

        <button
          onClick={() => zoomOut({ duration: 200 })}
          title="Zoom Out"
          className="p-1.5 text-zinc-400 hover:text-white rounded-xs hover:bg-zinc-800 transition-colors"
        >
          <ZoomOut className="w-3.5 h-3.5" />
        </button>

        <button
          onClick={() => fitView({ padding: 0.2, duration: 400 })}
          title="Fit to Screen"
          className="p-1.5 text-zinc-400 hover:text-cyan-400 rounded-xs hover:bg-zinc-800 transition-colors"
        >
          <Maximize className="w-3.5 h-3.5" />
        </button>

        <button
          onClick={handleResetView}
          title="Reset Center (0, 0)"
          className="p-1.5 text-zinc-400 hover:text-emerald-400 rounded-xs hover:bg-zinc-800 transition-colors"
        >
          <RotateCcw className="w-3.5 h-3.5" />
        </button>

        <div className="w-[1px] h-4 bg-zinc-800 my-auto mx-0.5" />

        <button
          onClick={() => setIsLocked(!isLocked)}
          title={isLocked ? 'Unlock Canvas Navigation' : 'Lock Canvas Navigation'}
          className={`p-1.5 rounded-xs transition-colors ${
            isLocked
              ? 'text-amber-400 bg-amber-950/60'
              : 'text-zinc-400 hover:text-white hover:bg-zinc-800'
          }`}
        >
          {isLocked ? <Lock className="w-3.5 h-3.5" /> : <Unlock className="w-3.5 h-3.5" />}
        </button>
      </div>

      {/* Legend Badge (Bottom Left) */}
      <div className="absolute bottom-3 left-3 z-20 flex items-center gap-3 bg-zinc-900/90 border border-zinc-800 px-2.5 py-1 rounded-xs shadow-lg backdrop-blur-xs font-mono text-[9px] text-zinc-400">
        <div className="flex items-center gap-1">
          <span className="w-2 h-2 rounded-full bg-cyan-400 shadow-[0_0_6px_#00f3ff]" />
          <span>Core Kernel</span>
        </div>
        <div className="flex items-center gap-1">
          <span className="w-2 h-2 rounded-sm bg-emerald-400 shadow-[0_0_6px_#10b981]" />
          <span>CRDT Shards</span>
        </div>
        <div className="flex items-center gap-1">
          <span className="w-2 h-2 rounded-full bg-indigo-400" />
          <span>Enclaves</span>
        </div>
        <div className="flex items-center gap-1">
          <span className="text-cyan-400">⚡</span>
          <span>A2A Message Beams</span>
        </div>
      </div>
    </div>
  );
}
