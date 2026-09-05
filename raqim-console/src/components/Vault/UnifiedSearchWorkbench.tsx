'use client';

import React, { useState } from 'react';
import { VaultSearchResult, formatTxIdHex } from '../../lib/api';
import {
  Search,
  Filter,
  Zap,
  Database,
  ShieldCheck,
  Copy,
  Check,
  ChevronRight,
} from 'lucide-react';

interface UnifiedSearchWorkbenchProps {
  results: VaultSearchResult[];
  onSearch: (query: string, namespace: string, includeWal: boolean) => Promise<void>;
  onSelectTxId: (txIdHex: string) => void;
  selectedTxIdHex: string | null;
  isLoading: boolean;
  namespaces: string[];
}

const getScoreColor = (score: number) => {
  if (score >= 0.85) {
    return {
      bg: 'bg-emerald-950/70 border-emerald-700/80',
      text: 'text-emerald-300',
      bar: 'bg-emerald-400',
    };
  }
  if (score >= 0.7) {
    return {
      bg: 'bg-amber-950/70 border-amber-700/80',
      text: 'text-amber-300',
      bar: 'bg-amber-400',
    };
  }
  return {
    bg: 'bg-zinc-900 border-zinc-700',
    text: 'text-zinc-300',
    bar: 'bg-zinc-500',
  };
};

const renderPayloadWithTags = (payload: string) => {
  const parts = payload.split(/(\[[A-Z_]+(?::\s*[^\]]+)?\])/g);

  return parts.map((part, i) => {
    if (part.startsWith('[') && part.endsWith(']')) {
      const tagContent = part.slice(1, -1);
      let tagClass = 'bg-zinc-800 text-zinc-300 border-zinc-700';

      if (tagContent.includes('REASONING')) {
        tagClass = 'bg-purple-950/80 text-purple-300 border-purple-800/80';
      } else if (tagContent.includes('TOOL_EXEC') || tagContent.includes('SQL')) {
        tagClass = 'bg-amber-950/80 text-amber-300 border-amber-800/80';
      } else if (tagContent.includes('HALT') || tagContent.includes('ERROR')) {
        tagClass = 'bg-rose-950/80 text-rose-300 border-rose-800/80';
      } else if (tagContent.includes('INJECT') || tagContent.includes('SYS')) {
        tagClass = 'bg-cyan-950/80 text-cyan-300 border-cyan-800/80';
      }

      return (
        <span
          key={i}
          className={`inline-block px-1.5 py-0.2 rounded-xs border font-mono text-[10px] font-bold mx-0.5 ${tagClass}`}
        >
          {part}
        </span>
      );
    }
    return <span key={i}>{part}</span>;
  });
};

export function UnifiedSearchWorkbench({
  results,
  onSearch,
  onSelectTxId,
  selectedTxIdHex,
  isLoading,
  namespaces,
}: UnifiedSearchWorkbenchProps) {
  const [query, setQuery] = useState('');
  const [namespace, setNamespace] = useState<string>('ALL');
  const [includeWal, setIncludeWal] = useState(true);
  const [copiedId, setCopiedId] = useState<string | null>(null);

  const handleSearchSubmit = (e?: React.FormEvent) => {
    if (e) e.preventDefault();
    if (!query.trim()) return;
    onSearch(query.trim(), namespace, includeWal);
  };

  const handleCopyHex = (e: React.MouseEvent, hex: string) => {
    e.stopPropagation();
    navigator.clipboard.writeText(hex);
    setCopiedId(hex);
    setTimeout(() => setCopiedId(null), 2000);
  };

  return (
    <div className="flex-1 flex flex-col min-h-0 bg-zinc-950 border border-zinc-800/80 rounded-sm overflow-hidden shadow-lg select-none">
      {/* Search Header Controls */}
      <form
        onSubmit={handleSearchSubmit}
        className="bg-zinc-900/90 border-b border-zinc-800 p-3 space-y-3 shrink-0"
      >
        <div className="flex items-center justify-between gap-2">
          <div className="flex items-center gap-2">
            <Search className="w-4 h-4 text-cyan-400" />
            <span className="font-sans text-xs uppercase tracking-wider font-bold text-white">
              Unified RAG &amp; Forensic Vault Search
            </span>
          </div>
          <span className="font-mono text-[10px] text-cyan-400 font-bold">
            HYBRID COSINE + LEXICAL
          </span>
        </div>

        {/* Query Input Bar */}
        <div className="relative">
          <input
            type="text"
            value={query}
            onChange={(e) => setQuery(e.target.value)}
            placeholder="Enter semantic concept, SQL query, or exact keyword..."
            disabled={isLoading}
            className="w-full pl-3 pr-32 py-2 bg-zinc-950 border border-zinc-800 focus:border-cyan-500/80 rounded-xs text-xs font-mono text-zinc-100 placeholder:text-zinc-500 outline-none transition-colors"
          />
          <button
            type="submit"
            disabled={isLoading || !query.trim()}
            className="absolute right-1.5 top-1/2 -translate-y-1/2 px-3 py-1 bg-cyan-950/90 hover:bg-cyan-900 border border-cyan-500/70 text-cyan-200 rounded-xs font-mono text-[11px] font-bold uppercase tracking-wider transition-all disabled:opacity-40"
          >
            {isLoading ? 'SEARCHING...' : 'EXECUTE SEARCH'}
          </button>
        </div>

        {/* Filter Controls Row */}
        <div className="flex flex-wrap items-center justify-between gap-3 text-xs font-mono pt-1">
          {/* Namespace Filter */}
          <div className="flex items-center gap-1.5">
            <Filter className="w-3 h-3 text-zinc-500" />
            <span className="text-zinc-400 text-[10px] uppercase font-sans font-bold">Namespace:</span>
            <select
              value={namespace}
              onChange={(e) => setNamespace(e.target.value)}
              className="bg-zinc-950 border border-zinc-800 rounded-xs px-2 py-0.5 text-[11px] text-zinc-200 outline-none cursor-pointer"
            >
              <option value="ALL">ALL NAMESPACES</option>
              {namespaces.map((ns) => (
                <option key={ns} value={ns}>
                  {ns}
                </option>
              ))}
            </select>
          </div>

          {/* Hot WAL Toggle Switch */}
          <label className="flex items-center gap-2 cursor-pointer select-none">
            <span className="text-zinc-400 text-[10px] uppercase font-sans font-bold">
              Include Hot WAL Buffer:
            </span>
            <button
              type="button"
              onClick={() => setIncludeWal(!includeWal)}
              className={`w-9 h-4.5 rounded-full transition-colors relative p-0.5 ${
                includeWal ? 'bg-amber-500' : 'bg-zinc-800'
              }`}
            >
              <div
                className={`w-3.5 h-3.5 rounded-full bg-zinc-950 transition-transform ${
                  includeWal ? 'translate-x-4.5' : 'translate-x-0'
                }`}
              />
            </button>
            <span className={`text-[10px] font-bold ${includeWal ? 'text-amber-400' : 'text-zinc-500'}`}>
              {includeWal ? 'ON' : 'OFF'}
            </span>
          </label>
        </div>
      </form>

      {/* Search Results Stream */}
      <div className="flex-1 overflow-y-auto p-3 space-y-2.5 bg-zinc-950 scrollbar-thin scrollbar-thumb-zinc-800">
        {isLoading ? (
          <div className="py-20 flex flex-col items-center justify-center gap-2 text-zinc-400 font-mono text-xs">
            <div className="w-6 h-6 border-2 border-cyan-400 border-t-transparent rounded-full animate-spin" />
            <span>SCATTER-GATHERING LANCEDB &amp; WAL BUFFERS...</span>
          </div>
        ) : results.length === 0 ? (
          <div className="py-20 flex flex-col items-center justify-center gap-2 text-zinc-400 font-mono text-xs uppercase tracking-wider text-center">
            <Database className="w-8 h-8 text-zinc-700" />
            <span>ENTER A QUERY TO SEARCH THE VECTOR AUDIT VAULT</span>
            <span className="text-[10px] text-zinc-400 normal-case">
              Targeting LanceDB Cold Storage &amp; Axon RAM Buffer with FastEmbed BGE-Base (768-dim).
            </span>
          </div>
        ) : (
          results.map((item, idx) => {
            const txHex = formatTxIdHex(item.tx_id);
            const isSelected = selectedTxIdHex === txHex;
            const score = item.similarity_score ?? item.score ?? 0;
            const scoreColor = getScoreColor(score);
            const isHotWal = item.source === 'HOT_WAL';
            const agentHex = item.agent_hex || item.agent_id || '0xUNKNOWN';
            const payloadContent = item.payload || item.text || '';
            const itemNamespace = item.namespace || '/rqm_core';

            return (
              <div
                key={`${item.tx_id}-${idx}`}
                onClick={() => onSelectTxId(txHex)}
                className={`bg-zinc-900/60 border rounded-sm p-3 space-y-2 cursor-pointer transition-all duration-150 group ${
                  isSelected
                    ? 'border-cyan-400 ring-1 ring-cyan-400/50 bg-zinc-900 shadow-[0_0_15px_rgba(0,243,255,0.2)]'
                    : 'border-zinc-800/80 hover:border-zinc-700 hover:bg-zinc-900/80'
                }`}
              >
                {/* Result Header: Score + Source + Namespace */}
                <div className="flex items-center justify-between gap-2 font-mono text-[11px]">
                  <div className="flex items-center gap-2">
                    {/* Score Badge */}
                    <span
                      className={`inline-flex items-center gap-1 px-2 py-0.5 rounded-xs border font-bold ${scoreColor.bg} ${scoreColor.text}`}
                    >
                      <span>{(score * 100).toFixed(1)}%</span>
                      <span className="text-[9px] uppercase font-sans">SIMILARITY</span>
                    </span>

                    {/* Source Badge */}
                    <span
                      className={`inline-flex items-center gap-1 px-2 py-0.5 rounded-xs border text-[10px] font-bold ${
                        isHotWal
                          ? 'bg-amber-950/70 border-amber-800/70 text-amber-300'
                          : 'bg-emerald-950/70 border-emerald-800/70 text-emerald-300'
                      }`}
                    >
                      {isHotWal ? <Zap className="w-3 h-3" /> : <Database className="w-3 h-3" />}
                      <span>{isHotWal ? 'HOT WAL' : 'COLD LANCEDB'}</span>
                    </span>
                  </div>

                  {/* Namespace */}
                  <span className="text-emerald-400 font-bold truncate max-w-xs" title={itemNamespace}>
                    {itemNamespace}
                  </span>
                </div>

                {/* Payload Preview */}
                <div className="text-zinc-200 text-xs font-mono leading-relaxed bg-zinc-950/80 p-2 rounded-xs border border-zinc-800">
                  {renderPayloadWithTags(payloadContent)}
                </div>

                {/* Result Footer: TxID + Timestamp + Inspect Prompt */}
                <div className="flex items-center justify-between pt-1 border-t border-zinc-800/80 font-mono text-[10px] text-zinc-400">
                  <div className="flex items-center gap-3">
                    {/* TxID copy */}
                    <button
                      onClick={(e) => handleCopyHex(e, txHex)}
                      className="flex items-center gap-1 text-cyan-400 hover:text-cyan-300 font-bold transition-colors"
                      title={`Copy TxID: ${txHex}`}
                    >
                      <span>TX: {txHex.slice(0, 8)}...{txHex.slice(-6)}</span>
                      {copiedId === txHex ? (
                        <Check className="w-2.5 h-2.5 text-emerald-400" />
                      ) : (
                        <Copy className="w-2.5 h-2.5 text-zinc-500" />
                      )}
                    </button>

                    <span>AGENT: {agentHex.slice(0, 6)}...</span>
                  </div>

                  <div className="flex items-center gap-1.5 text-zinc-400 group-hover:text-cyan-300 transition-colors">
                    <ShieldCheck className="w-3 h-3 text-cyan-400" />
                    <span>INSPECT MERKLE PROOF</span>
                    <ChevronRight className="w-3 h-3" />
                  </div>
                </div>
              </div>
            );
          })
        )}
      </div>
    </div>
  );
}
