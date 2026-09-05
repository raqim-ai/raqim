'use client';

import React, { useState, useMemo } from 'react';
import { ClusterShard, formatTxIdHex } from '../../lib/api';
import { useSwarmStore } from '../../lib/store/useSwarmStore';
import {
  Database,
  Layers,
  Activity,
  HardDrive,
  Bot,
  Terminal,
  X,
  Copy,
  Check,
  Zap,
} from 'lucide-react';

interface ShardDetailDrawerProps {
  shard: ClusterShard | null;
  onClose: () => void;
}

export function ShardDetailDrawer({ shard, onClose }: ShardDetailDrawerProps) {
  const thoughts = useSwarmStore((state) => state.thoughts);
  const thoughtOrder = useSwarmStore((state) => state.thoughtOrder);
  const agentAliases = useSwarmStore((state) => state.agentAliases);
  const agentLastSeen = useSwarmStore((state) => state.agentLastSeen);
  const quarantinedAgents = useSwarmStore((state) => state.quarantinedAgents);

  const [copiedId, setCopiedId] = useState<string | null>(null);

  if (!shard) return null;

  const namespace = shard.namespace;
  const operations = shard.total_crdt_operations ?? shard.total_crdt_operation ?? 0;
  const timelines = shard.active_timelines ?? 1;
  const estimatedRamMb = ((operations * 0.0008) + 0.12).toFixed(2);

  // Filter recent thoughts for this specific shard namespace
  const namespaceThoughts = useMemo(() => {
    const list = [];
    for (let i = thoughtOrder.length - 1; i >= 0; i--) {
      const t = thoughts[thoughtOrder[i]];
      if (t && t.intent_path === namespace) {
        list.push(t);
        if (list.length >= 20) break;
      }
    }
    return list;
  }, [thoughts, thoughtOrder, namespace]);

  // Find all attached agents
  const attachedAgents = useMemo(() => {
    const map = new Map<string, { hex: string; alias: string; isLive: boolean; isQuarantined: boolean }>();

    for (const [hex, alias] of Object.entries(agentAliases)) {
      if (namespace === '/default' || namespace === '/siege/shard_00') {
        const isLive = Date.now() - (agentLastSeen[hex] || 0) < 60000;
        const isQuarantined = quarantinedAgents.includes(hex);
        map.set(hex, { hex, alias, isLive, isQuarantined });
      }
    }

    for (const t of namespaceThoughts) {
      if (!map.has(t.agent_hex)) {
        const isLive = Date.now() - (agentLastSeen[t.agent_hex] || 0) < 60000;
        const isQuarantined = quarantinedAgents.includes(t.agent_hex);
        const alias = agentAliases[t.agent_hex] || `agent_${t.agent_hex.slice(0, 6)}`;
        map.set(t.agent_hex, { hex: t.agent_hex, alias, isLive, isQuarantined });
      }
    }

    return Array.from(map.values());
  }, [agentAliases, agentLastSeen, quarantinedAgents, namespace, namespaceThoughts]);

  const handleCopyHex = (hex: string) => {
    navigator.clipboard.writeText(hex);
    setCopiedId(hex);
    setTimeout(() => setCopiedId(null), 2000);
  };

  return (
    <aside className="fixed top-0 right-0 bottom-0 w-full sm:w-[480px] bg-zinc-950 border-l border-zinc-800 shadow-2xl z-40 flex flex-col animate-in slide-in-from-right duration-200 select-none">
      {/* Header */}
      <div className="bg-zinc-900/90 border-b border-zinc-800 p-3.5 flex items-center justify-between select-none shrink-0">
        <div className="flex items-center gap-2 min-w-0">
          <Database className="w-4 h-4 text-emerald-400 shrink-0" />
          <span className="font-sans text-xs uppercase tracking-wider font-bold text-white truncate">
            LORO CRDT SHARD: <span className="text-emerald-400">{namespace}</span>
          </span>
        </div>
        <button
          onClick={onClose}
          className="p-1 text-zinc-400 hover:text-white rounded-xs hover:bg-zinc-800 transition-colors shrink-0"
        >
          <X className="w-4 h-4" />
        </button>
      </div>

      {/* Body Content */}
      <div className="flex-1 overflow-y-auto p-4 space-y-4 font-mono text-xs scrollbar-thin scrollbar-thumb-zinc-800">
        {/* Memory Partition Metrology */}
        <div className="bg-zinc-900/60 border border-zinc-800 rounded-sm p-3 space-y-2.5">
          <div className="flex items-center justify-between text-zinc-400 text-[10px] font-sans uppercase font-bold tracking-wider">
            <span>Memory Partition Metrology</span>
            <span className="text-emerald-400 font-bold">SYNCED</span>
          </div>

          <div className="grid grid-cols-3 gap-2 text-[11px]">
            <div className="bg-zinc-950 p-2 rounded-xs border border-zinc-800/80">
              <div className="flex items-center gap-1 text-zinc-400 text-[9px] uppercase font-sans mb-0.5 font-bold">
                <Layers className="w-2.5 h-2.5 text-cyan-400" />
                <span>Timelines</span>
              </div>
              <span className="font-bold text-white text-sm">{timelines}</span>
            </div>

            <div className="bg-zinc-950 p-2 rounded-xs border border-zinc-800/80">
              <div className="flex items-center gap-1 text-zinc-400 text-[9px] uppercase font-sans mb-0.5 font-bold">
                <Activity className="w-2.5 h-2.5 text-purple-400" />
                <span>CRDT Ops</span>
              </div>
              <span className="font-bold text-purple-300 text-sm">{operations.toLocaleString()}</span>
            </div>

            <div className="bg-zinc-950 p-2 rounded-xs border border-zinc-800/80">
              <div className="flex items-center gap-1 text-zinc-400 text-[9px] uppercase font-sans mb-0.5 font-bold">
                <HardDrive className="w-2.5 h-2.5 text-amber-400" />
                <span>Est. RAM</span>
              </div>
              <span className="font-bold text-amber-300 text-sm">{estimatedRamMb} MB</span>
            </div>
          </div>
        </div>

        {/* Attached Enclave Agents */}
        <div className="space-y-2">
          <div className="flex items-center justify-between text-zinc-400 text-[10px] font-sans uppercase font-bold tracking-wider">
            <div className="flex items-center gap-1.5">
              <Bot className="w-3 h-3 text-cyan-400" />
              <span>Active Attached Agents ({attachedAgents.length})</span>
            </div>
          </div>

          <div className="bg-zinc-900/60 border border-zinc-800 rounded-sm overflow-hidden max-h-48 overflow-y-auto">
            {attachedAgents.length === 0 ? (
              <div className="p-3 text-center text-zinc-500 text-[10px] uppercase">
                [AGENT ACTIVE IN SHARD]
              </div>
            ) : (
              <table className="w-full text-left text-[10px]">
                <thead className="bg-zinc-900 text-zinc-400 border-b border-zinc-800 font-sans text-[9px] uppercase">
                  <tr>
                    <th className="py-1 px-2.5">ALIAS</th>
                    <th className="py-1 px-2.5">IDENTITY</th>
                    <th className="py-1 px-2.5 text-right">STATUS</th>
                  </tr>
                </thead>
                <tbody className="divide-y divide-zinc-800/40">
                  {attachedAgents.map((ag) => (
                    <tr key={ag.hex} className="hover:bg-zinc-900/50">
                      <td className="py-1.5 px-2.5 font-bold text-white truncate max-w-[120px]">
                        [{ag.alias}]
                      </td>
                      <td className="py-1.5 px-2.5">
                        <button
                          onClick={() => handleCopyHex(ag.hex)}
                          className="flex items-center gap-1 text-cyan-400 hover:text-cyan-300"
                        >
                          <span>{ag.hex.slice(0, 6)}...</span>
                          {copiedId === ag.hex ? (
                            <Check className="w-2.5 h-2.5 text-emerald-400" />
                          ) : (
                            <Copy className="w-2.5 h-2.5 text-zinc-500" />
                          )}
                        </button>
                      </td>
                      <td className="py-1.5 px-2.5 text-right">
                        <span
                          className={`font-semibold ${
                            ag.isQuarantined
                              ? 'text-rose-400'
                              : ag.isLive
                              ? 'text-emerald-400'
                              : 'text-zinc-400'
                          }`}
                        >
                          {ag.isQuarantined ? 'Quarantined' : ag.isLive ? 'Active' : 'Idle'}
                        </span>
                      </td>
                    </tr>
                  ))}
                </tbody>
              </table>
            )}
          </div>
        </div>

        {/* Recent Namespace Thoughts Feed */}
        <div className="space-y-2">
          <div className="flex items-center justify-between text-zinc-400 text-[10px] font-sans uppercase font-bold tracking-wider">
            <div className="flex items-center gap-1.5">
              <Terminal className="w-3 h-3 text-cyan-400" />
              <span>Recent Partition Events (Last 20)</span>
            </div>
            <span className="text-cyan-400 font-mono text-[9px] font-bold">LIVE FEED</span>
          </div>

          <div className="bg-zinc-900/60 border border-zinc-800 rounded-sm p-2 space-y-1.5 max-h-60 overflow-y-auto">
            {namespaceThoughts.length === 0 ? (
              <div className="p-4 text-center text-zinc-500 text-[10px]">
                AWAITING LIVE THOUGHTS IN {namespace}...
              </div>
            ) : (
              namespaceThoughts.map((t) => {
                const txHex = t.tx_id_hex || formatTxIdHex(t.tx_id);
                return (
                  <div
                    key={t.tx_id}
                    className="p-2 bg-zinc-950 border border-zinc-800/60 rounded-xs space-y-1"
                  >
                    <div className="flex items-center justify-between text-[9px] text-zinc-400">
                      <span className="text-cyan-400 font-bold">
                        TX: {txHex.slice(0, 8)}...
                      </span>
                      <span className="text-zinc-500">
                        {t.timestamp ? new Date(t.timestamp).toLocaleTimeString() : ''}
                      </span>
                    </div>
                    <p className="text-zinc-200 text-[10px] truncate leading-tight">
                      {t.text}
                    </p>
                  </div>
                );
              })
            )}
          </div>
        </div>
      </div>

      {/* Footer */}
      <div className="bg-zinc-900/90 border-t border-zinc-800 p-3 flex justify-end shrink-0">
        <button
          onClick={onClose}
          className="px-3.5 py-1.5 rounded-xs bg-zinc-800 hover:bg-zinc-700 border border-zinc-700 text-zinc-200 font-sans text-xs font-semibold transition-colors"
        >
          Close Forensic View
        </button>
      </div>
    </aside>
  );
}
