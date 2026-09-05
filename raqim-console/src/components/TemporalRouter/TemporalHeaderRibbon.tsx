'use client';

import React, { useState } from 'react';
import { ClusterEnclave } from '../../lib/api';
import { Bot, Copy, Check, Zap, Play, Radio, Sparkles } from 'lucide-react';

interface TemporalHeaderRibbonProps {
  enclaves: ClusterEnclave[];
  agentAliases: Record<string, string>;
  selectedAgentHex: string;
  onSelectAgent: (hex: string) => void;
  mode: 'RECORD' | 'REPLAY';
  activeTxIdHex: string | null;
  isPlaying?: boolean;
  onTogglePlay?: () => void;
}

export function TemporalHeaderRibbon({
  enclaves,
  agentAliases,
  selectedAgentHex,
  onSelectAgent,
  mode,
  activeTxIdHex,
  isPlaying,
  onTogglePlay,
}: TemporalHeaderRibbonProps) {
  const [copiedTx, setCopiedTx] = useState(false);

  const rawTxHex = activeTxIdHex || '0x00000000000000000000000000000000';
  const cleanHex = rawTxHex.startsWith('0x') ? rawTxHex : `0x${rawTxHex}`;
  const truncatedTx = `${cleanHex.slice(0, 18)}...`;

  const handleCopyTx = () => {
    navigator.clipboard.writeText(cleanHex);
    setCopiedTx(true);
    setTimeout(() => setCopiedTx(false), 2000);
  };

  // Build target agent options list
  const agentOptions = React.useMemo(() => {
    if (enclaves && enclaves.length > 0) {
      return enclaves.map((enc) => {
        const hex = enc.identity_hex;
        const alias = agentAliases[hex] || enc.alias || `agent_${hex.slice(0, 6)}`;
        return {
          hex,
          label: `${alias} [${hex.slice(0, 8)}...]`,
        };
      });
    }

    const entries = Object.entries(agentAliases);
    if (entries.length > 0) {
      return entries.map(([hex, alias]) => ({
        hex,
        label: `${alias} [${hex.slice(0, 8)}...]`,
      }));
    }

    return [
      {
        hex: '096da8e8a1b2c3d4e5f60718293a4b5c',
        label: 'default_agent [096da8e8...]',
      },
    ];
  }, [enclaves, agentAliases]);

  return (
    <header className="bg-zinc-950/80 border border-zinc-800/80 rounded-sm p-3 flex flex-wrap items-center justify-between gap-3 shrink-0 shadow-lg select-none">
      {/* Left: Target Agent Dropdown & Mode Badge */}
      <div className="flex flex-wrap items-center gap-3">
        {/* Target Agent Selector */}
        <div className="flex items-center gap-2 bg-zinc-900 border border-zinc-800 rounded-xs px-2.5 py-1.5">
          <Bot className="w-4 h-4 text-cyan-400 shrink-0" />
          <span className="text-[10px] font-sans uppercase font-bold text-zinc-400">Target Agent:</span>
          <select
            value={selectedAgentHex}
            onChange={(e) => onSelectAgent(e.target.value)}
            className="bg-transparent text-xs font-mono font-bold text-cyan-300 outline-none cursor-pointer"
          >
            {agentOptions.map((opt) => (
              <option key={opt.hex} value={opt.hex} className="bg-zinc-950 text-zinc-100">
                {opt.label}
              </option>
            ))}
          </select>
        </div>

        {/* Mode Badge */}
        <div className="flex items-center gap-1.5 font-mono text-[11px] font-bold">
          {mode === 'RECORD' ? (
            <div className="flex items-center gap-1.5 px-2.5 py-1 rounded-xs bg-emerald-500/10 border border-emerald-500/30 text-emerald-400">
              <span className="w-2 h-2 rounded-full bg-emerald-400 animate-pulse" />
              <span>RECORD MODE (CANONICAL)</span>
            </div>
          ) : (
            <div className="flex items-center gap-1.5 px-2.5 py-1 rounded-xs bg-amber-500/10 border border-amber-500/30 text-amber-400">
              <Radio className="w-3.5 h-3.5 text-amber-400 animate-pulse" />
              <span>REPLAY (TIME TRAVEL)</span>
            </div>
          )}
        </div>
      </div>

      {/* Right: Causal Tx Badge & Script Replay Info Pill */}
      <div className="flex flex-wrap items-center gap-2.5">
        {/* Causal Tx Badge */}
        <div className="flex items-center gap-1.5 bg-zinc-900 border border-zinc-800 rounded-xs px-2.5 py-1 font-mono text-[11px]">
          <span className="text-zinc-400 text-[10px] uppercase font-sans font-bold">CAUSAL TX:</span>
          <button
            onClick={handleCopyTx}
            title={`Copy full TxID: ${cleanHex}`}
            className="flex items-center gap-1 text-cyan-400 hover:text-cyan-300 font-bold transition-colors"
          >
            <span>{truncatedTx}</span>
            {copiedTx ? (
              <Check className="w-3 h-3 text-emerald-400" />
            ) : (
              <Copy className="w-3 h-3 text-zinc-500 hover:text-zinc-300" />
            )}
          </button>
        </div>

        {/* Replay Ready Info Pill */}
        <div className="flex items-center gap-1.5 px-3 py-1 rounded-xs bg-emerald-950/60 border border-emerald-800/80 text-emerald-300 font-mono text-xs font-bold tracking-tight">
          <Zap className="w-3.5 h-3.5 text-emerald-400" />
          <span>[⚡ REPLAY READY - RUN SCRIPT WITH mode=&quot;replay&quot;]</span>
        </div>
      </div>
    </header>
  );
}
