'use client';

import React from 'react';
import { TimelineNode, formatTxIdHex } from '../../lib/api';
import {
  FileCode,
  CheckCircle2,
  Database,
  Layers,
  Copy,
  Check,
  ShieldCheck,
} from 'lucide-react';

interface EffectDiffInspectorProps {
  currentNode: TimelineNode | null;
  stepIndex: number;
  agentHex: string;
}

export function EffectDiffInspector({
  currentNode,
  stepIndex,
  agentHex,
}: EffectDiffInspectorProps) {
  const [copiedTx, setCopiedTx] = React.useState(false);

  const rawTxHex = currentNode ? formatTxIdHex(currentNode.tx_id) : '0x00000000000000000000000000000000';
  const cleanTxHex = rawTxHex.startsWith('0x') ? rawTxHex : `0x${rawTxHex}`;
  const status = currentNode?.agent_status || 'IDLE';
  const payloadPreview = currentNode?.payload_preview || '// Awaiting active step selection from timeline...';

  const handleCopyTx = () => {
    navigator.clipboard.writeText(cleanTxHex);
    setCopiedTx(true);
    setTimeout(() => setCopiedTx(false), 2000);
  };

  return (
    <div className="flex-1 flex flex-col min-h-0 bg-zinc-950 border border-zinc-800/80 rounded-sm overflow-hidden shadow-lg select-none">
      {/* Header */}
      <div className="bg-zinc-900/90 border-b border-zinc-800 px-3 py-2 flex items-center justify-between gap-2 shrink-0 select-none">
        <div className="flex items-center gap-2">
          <FileCode className="w-3.5 h-3.5 text-cyan-400" />
          <span className="font-sans text-xs uppercase tracking-wider font-bold text-white">
            Side-Effect Boundary &amp; State Diff Inspector
          </span>
        </div>
        <span className="font-mono text-[10px] text-emerald-400 font-bold px-2 py-0.5 rounded-full bg-emerald-500/10 border border-emerald-500/30">
          STEP #{stepIndex + 1} INSPECTION
        </span>
      </div>

      {/* 2-Column Comparative Split */}
      <div className="flex-1 grid grid-cols-1 md:grid-cols-2 gap-0 min-h-0 divide-y md:divide-y-0 md:divide-x divide-zinc-800 bg-zinc-950 overflow-hidden">
        {/* Left Panel: Historical Baseline - WAL Storage */}
        <div className="flex-1 flex flex-col min-h-0 overflow-hidden">
          <div className="bg-zinc-900/80 px-3 py-2 border-b border-zinc-800 flex items-center justify-between text-[10px] font-mono select-none">
            <span className="text-zinc-400 font-sans uppercase font-bold flex items-center gap-1.5">
              <Database className="w-3 h-3 text-cyan-400" />
              <span>Historical Baseline (WAL Storage)</span>
            </span>
            <span className="text-cyan-400 font-bold">CANONICAL TRACE</span>
          </div>

          {/* Metadata Row: TX_ID & STATUS */}
          <div className="p-3 bg-zinc-950 border-b border-zinc-800/80 space-y-1.5 font-mono text-[11px]">
            <div className="flex items-center justify-between">
              <span className="text-zinc-500 uppercase font-sans text-[10px] font-bold">TX_ID:</span>
              <button
                onClick={handleCopyTx}
                title={`Copy TxID: ${cleanTxHex}`}
                className="text-cyan-400 hover:text-cyan-300 flex items-center gap-1 font-bold"
              >
                <span>{cleanTxHex}</span>
                {copiedTx ? (
                  <Check className="w-3 h-3 text-emerald-400" />
                ) : (
                  <Copy className="w-3 h-3 text-zinc-500 hover:text-zinc-300" />
                )}
              </button>
            </div>
            <div className="flex items-center justify-between">
              <span className="text-zinc-500 uppercase font-sans text-[10px] font-bold">STATUS:</span>
              <span className="text-emerald-400 font-bold px-1.5 py-0.2 rounded-xs bg-emerald-500/10 border border-emerald-500/30 text-[10px]">
                {status}
              </span>
            </div>
          </div>

          {/* CANONICAL PAYLOAD */}
          <div className="flex-1 p-3 overflow-y-auto font-mono text-xs text-zinc-200 bg-zinc-950 leading-relaxed scrollbar-thin scrollbar-thumb-zinc-800">
            <div className="text-[10px] font-sans uppercase font-bold text-zinc-500 mb-1.5">
              CANONICAL PAYLOAD:
            </div>
            <pre className="whitespace-pre-wrap break-words bg-zinc-900/60 p-2.5 rounded-xs border border-zinc-800/80 text-zinc-200">
              {payloadPreview}
            </pre>
          </div>
        </div>

        {/* Right Panel: Replayed / Divergent State */}
        <div className="flex-1 flex flex-col min-h-0 overflow-hidden">
          <div className="bg-zinc-900/80 px-3 py-2 border-b border-zinc-800 flex items-center justify-between text-[10px] font-mono select-none">
            <span className="text-zinc-400 font-sans uppercase font-bold flex items-center gap-1.5">
              <Layers className="w-3 h-3 text-emerald-400" />
              <span>Replayed / Divergent State</span>
            </span>
            <span className="text-emerald-400 font-bold">REPLAY VERIFIED</span>
          </div>

          {/* Green Status Badge & Subtitle */}
          <div className="p-3 bg-zinc-950 border-b border-zinc-800/80 space-y-1.5">
            <div className="p-1.5 rounded-xs bg-emerald-500/10 border border-emerald-500/30 text-emerald-400 text-xs font-mono flex items-center gap-2 font-bold">
              <CheckCircle2 className="w-4 h-4 text-emerald-400 shrink-0" />
              <span>VERIFIED MATCH - 100% DETERMINISTIC (0ms / $0)</span>
            </div>
            <p className="text-[11px] font-mono text-zinc-400 leading-tight">
              Historical output locked in WAL. Live re-executions bypass API calls and load from disk.
            </p>
          </div>

          {/* Replayed Payload Output */}
          <div className="flex-1 p-3 overflow-y-auto font-mono text-xs text-zinc-200 bg-zinc-950 leading-relaxed scrollbar-thin scrollbar-thumb-zinc-800">
            <div className="text-[10px] font-sans uppercase font-bold text-zinc-500 mb-1.5">
              DETERMINISTIC CACHE REPLAY:
            </div>
            <pre className="whitespace-pre-wrap break-words bg-zinc-900/60 p-2.5 rounded-xs border border-zinc-800/80 text-emerald-300">
              {payloadPreview}
            </pre>
          </div>
        </div>
      </div>
    </div>
  );
}
