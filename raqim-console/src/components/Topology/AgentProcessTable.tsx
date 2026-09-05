'use client';

import React, { useState, useMemo } from 'react';
import { ClusterEnclave } from '../../lib/api';
import { useSwarmStore } from '../../lib/store/useSwarmStore';
import { Bot, Search, Copy, Check, ShieldAlert, Radio } from 'lucide-react';

interface AgentProcessTableProps {
  enclaves: ClusterEnclave[];
  agentAliases?: Record<string, string>;
}

const getAgentColor = (hex: string) => {
  let hash = 0;
  for (let i = 0; i < hex.length; i++) {
    hash = hex.charCodeAt(i) + ((hash << 5) - hash);
  }
  const colors = [
    'bg-cyan-500/10 text-cyan-400 border-cyan-500/30',
    'bg-emerald-500/10 text-emerald-400 border-emerald-500/30',
    'bg-indigo-500/10 text-indigo-400 border-indigo-500/30',
    'bg-purple-500/10 text-purple-400 border-purple-500/30',
    'bg-amber-500/10 text-amber-400 border-amber-500/30',
    'bg-sky-500/10 text-sky-400 border-sky-500/30',
  ];
  return colors[Math.abs(hash) % colors.length];
};

export function AgentProcessTable({ enclaves, agentAliases = {} }: AgentProcessTableProps) {
  const quarantinedAgents = useSwarmStore((state) => state.quarantinedAgents);
  const agentLastSeen = useSwarmStore((state) => state.agentLastSeen);

  const [searchQuery, setSearchQuery] = useState('');
  const [copiedId, setCopiedId] = useState<string | null>(null);

  // Reconcile enclaves with live aliases and last seen states
  const enclaveList = useMemo(() => {
    if (enclaves && enclaves.length > 0) {
      return enclaves.map((enc) => {
        const isQuarantined = quarantinedAgents.includes(enc.identity_hex);
        const resolvedAlias =
          agentAliases[enc.identity_hex] ||
          (enc.alias === 'Unknown' ? enc.alias : enc.alias);
        return {
          ...enc,
          alias: resolvedAlias,
          isQuarantined,
        };
      });
    }

    // Fallback if enclaves array is empty
    return Object.entries(agentAliases).map(([hex, alias], idx) => ({
      alias: alias === 'Unknown' ? `agent_shard_${String(idx).padStart(2, '0')}` : alias,
      identity_hex: hex,
      home_shard: `/siege/shard_${String(idx).padStart(2, '0')}`,
      status: 'IDLE_IN_RAM',
      last_seen_ts: agentLastSeen[hex] || 0,
      isQuarantined: quarantinedAgents.includes(hex),
    }));
  }, [enclaves, agentAliases, quarantinedAgents, agentLastSeen]);

  const filteredEnclaves = useMemo(() => {
    if (!searchQuery.trim()) return enclaveList;
    const q = searchQuery.toLowerCase().trim();
    return enclaveList.filter(
      (e) =>
        e.alias.toLowerCase().includes(q) ||
        e.identity_hex.toLowerCase().includes(q) ||
        e.home_shard.toLowerCase().includes(q)
    );
  }, [enclaveList, searchQuery]);

  const handleCopyHex = (hex: string) => {
    const fullHex = hex.startsWith('0x') ? hex : `0x${hex}`;
    navigator.clipboard.writeText(fullHex);
    setCopiedId(hex);
    setTimeout(() => setCopiedId(null), 2000);
  };

  return (
    <div className="bg-zinc-950/80 border border-zinc-800/80 rounded-sm overflow-hidden flex flex-col shadow-lg">
      {/* Header & Filter */}
      <div className="bg-zinc-900/90 border-b border-zinc-800 px-3 py-2 flex flex-wrap items-center justify-between gap-2 shrink-0 select-none">
        <div className="flex items-center gap-2">
          <Bot className="w-3.5 h-3.5 text-cyan-400" />
          <span className="font-sans text-xs uppercase tracking-wider font-bold text-zinc-300">
            Swarm Enclave Matrix &amp; CRDT Peer Processes
          </span>
          <span className="px-1.5 py-0.5 rounded-xs bg-zinc-800 border border-zinc-700 font-mono text-[10px] text-cyan-400 font-bold">
            {enclaveList.length} PEERS
          </span>
        </div>

        {/* Search Box */}
        <div className="relative">
          <Search className="w-3 h-3 text-zinc-500 absolute left-2 top-1/2 -translate-y-1/2 pointer-events-none" />
          <input
            type="text"
            value={searchQuery}
            onChange={(e) => setSearchQuery(e.target.value)}
            placeholder="Filter alias, hex ID, shard..."
            className="pl-6.5 pr-2 py-1 bg-zinc-950 border border-zinc-800 focus:border-cyan-500/60 rounded-xs text-[11px] font-mono text-zinc-200 placeholder:text-zinc-500 outline-none w-56 transition-colors"
          />
        </div>
      </div>

      {/* Table */}
      <div className="overflow-x-auto max-h-56 overflow-y-auto bg-zinc-950 scrollbar-thin scrollbar-thumb-zinc-800">
        <table className="w-full text-left border-collapse font-mono text-[11px]">
          <thead className="sticky top-0 z-10 bg-zinc-900 border-b border-zinc-800 text-zinc-400 font-sans text-[10px] uppercase tracking-wider select-none">
            <tr>
              <th className="py-2 px-3 w-44">AGENT ALIAS</th>
              <th className="py-2 px-3 w-56">IDENTITY HEX</th>
              <th className="py-2 px-3">HOME CRDT SHARD</th>
              <th className="py-2 px-3 w-36 text-right">STATUS</th>
            </tr>
          </thead>
          <tbody className="divide-y divide-zinc-800/50">
            {filteredEnclaves.length === 0 ? (
              <tr>
                <td colSpan={4} className="text-center py-8 text-zinc-500 text-xs font-mono uppercase">
                  {enclaveList.length === 0
                    ? '[ ZERO AGENT ENCLAVES DEPLOYED ]'
                    : `NO AGENTS MATCH FILTER "${searchQuery}"`}
                </td>
              </tr>
            ) : (
              filteredEnclaves.map((enc) => {
                const badgeColor = getAgentColor(enc.identity_hex);
                const fullHex = enc.identity_hex.startsWith('0x') ? enc.identity_hex : `0x${enc.identity_hex}`;
                const truncatedHex = `${fullHex.slice(0, 16)}...`;

                const isQuarantined = enc.isQuarantined;
                const statusNormalized = enc.status?.toUpperCase() || 'IDLE_IN_RAM';
                const isActive = statusNormalized === 'ACTIVE' || statusNormalized === 'LIVE';

                return (
                  <tr key={enc.identity_hex} className="hover:bg-zinc-900/60 transition-colors">
                    {/* 1. AGENT ALIAS */}
                    <td className="py-2 px-3 whitespace-nowrap">
                      <span
                        className={`inline-block px-1.5 py-0.5 rounded-xs border text-[10px] font-bold font-mono ${badgeColor}`}
                      >
                        [{enc.alias}]
                      </span>
                    </td>

                    {/* 2. IDENTITY HEX */}
                    <td className="py-2 px-3 whitespace-nowrap">
                      <button
                        onClick={() => handleCopyHex(enc.identity_hex)}
                        title={`Copy Hex ID: ${fullHex}`}
                        className="flex items-center gap-1.5 text-cyan-400 hover:text-cyan-300 font-bold transition-colors"
                      >
                        <span>{truncatedHex}</span>
                        {copiedId === enc.identity_hex ? (
                          <Check className="w-3 h-3 text-emerald-400" />
                        ) : (
                          <Copy className="w-3 h-3 text-zinc-500 hover:text-zinc-300" />
                        )}
                      </button>
                    </td>

                    {/* 3. HOME CRDT SHARD */}
                    <td className="py-2 px-3 whitespace-nowrap">
                      <span className="inline-block px-1.5 py-0.5 rounded-xs bg-zinc-900 border border-zinc-800 text-emerald-400 font-bold text-[10px]">
                        {enc.home_shard}
                      </span>
                    </td>

                    {/* 4. STATUS */}
                    <td className="py-2 px-3 whitespace-nowrap text-right">
                      <span
                        className={`inline-flex items-center gap-1 px-2 py-0.5 rounded-xs border text-[9px] font-bold uppercase tracking-wider ${
                          isQuarantined
                            ? 'bg-rose-950/70 border-rose-700/60 text-rose-300'
                            : isActive
                            ? 'bg-emerald-500/10 border-emerald-500/30 text-emerald-400'
                            : 'bg-zinc-900 border-zinc-800 text-zinc-400'
                        }`}
                      >
                        {isQuarantined ? (
                          <>
                            <ShieldAlert className="w-2.5 h-2.5 text-rose-400" />
                            <span>Quarantined</span>
                          </>
                        ) : isActive ? (
                          <>
                            <span className="w-1.5 h-1.5 rounded-full bg-emerald-400 animate-pulse" />
                            <span>Active</span>
                          </>
                        ) : (
                          <>
                            <span className="w-1.5 h-1.5 rounded-full bg-zinc-500" />
                            <span>IDLE_IN_RAM</span>
                          </>
                        )}
                      </span>
                    </td>
                  </tr>
                );
              })
            )}
          </tbody>
        </table>
      </div>
    </div>
  );
}
