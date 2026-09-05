'use client';

import React, { useState } from 'react';
import { useSwarmStore, formatTxIdHex } from '../../lib/store/useSwarmStore';
import { Activity, Copy, Check, Pause, Play, Server, Radio } from 'lucide-react';

export function GlobalStatusBar() {
  const daemonOnline = useSwarmStore((state) => state.daemonOnline);
  const currentTps = useSwarmStore((state) => state.currentTps);
  const latestTxIdHex = useSwarmStore((state) => state.latestTxIdHex);
  const highestTxId = useSwarmStore((state) => state.highestTxId);
  const dashboardCards = useSwarmStore((state) => state.dashboardCards);
  const clusterInfo = useSwarmStore((state) => state.clusterInfo);
  const isPaused = useSwarmStore((state) => state.isPaused);
  const togglePause = useSwarmStore((state) => state.togglePause);
  const setIsPaused = useSwarmStore((state) => state.setIsPaused);

  const [copiedTx, setCopiedTx] = useState(false);
  const [copiedNode, setCopiedNode] = useState(false);
  const [isToggling, setIsToggling] = useState(false);

  // 1. Latest Tx ID Resolution
  const rawTxHex =
    dashboardCards?.latest_tx_hex ||
    (latestTxIdHex && latestTxIdHex !== '0x00000000000000000000000000000000'
      ? latestTxIdHex
      : highestTxId > 0
      ? formatTxIdHex(highestTxId)
      : '0x00000000000000000000000000000000');

  const displayTxHex = rawTxHex.startsWith('0x') ? rawTxHex : `0x${rawTxHex}`;
  const truncatedTx =
    displayTxHex.length > 14
      ? `${displayTxHex.slice(0, 6)}...${displayTxHex.slice(-4)}`
      : displayTxHex;

  // Node ID Resolution
  const rawNodeId = clusterInfo?.node_id || 'LOCAL-DAEMON';
  const truncatedNodeId = rawNodeId.length > 14 ? `${rawNodeId.slice(0, 12)}...` : rawNodeId;

  const handleCopyTx = () => {
    if (!displayTxHex) return;
    navigator.clipboard.writeText(displayTxHex);
    setCopiedTx(true);
    setTimeout(() => setCopiedTx(false), 2000);
  };

  const handleCopyNode = () => {
    navigator.clipboard.writeText(rawNodeId);
    setCopiedNode(true);
    setTimeout(() => setCopiedNode(false), 2000);
  };

  // Ingress Toggle Handler
  const handleToggleIngress = async () => {
    setIsToggling(true);
    try {
      const res = await fetch('http://localhost:8081/v1/admin/ingress/toggle', {
        method: 'POST',
      });
      if (res.ok) {
        const data = await res.json();
        if (typeof data.is_ingress_paused === 'boolean') {
          setIsPaused(data.is_ingress_paused);
          setIsToggling(false);
          return;
        }
      }
    } catch {
      // Local fallback
    }
    togglePause();
    setIsToggling(false);
  };

  const ingressPaused = isPaused || dashboardCards?.ingress_paused === true;

  return (
    <header className="w-full bg-zinc-950 border-b border-zinc-800/80 px-4 py-2.5 flex flex-wrap items-center justify-between gap-3 text-xs shrink-0 select-none">
      {/* Left: Connectivity Beacon & Node ID */}
      <div className="flex items-center gap-3">
        {/* Daemon Beacon */}
        <div
          className={`flex items-center gap-2 px-2.5 py-1 rounded-xs border font-mono text-[11px] uppercase tracking-wider font-semibold transition-all ${
            daemonOnline
              ? 'bg-emerald-500/10 border-emerald-500/30 text-emerald-400 shadow-[0_0_10px_rgba(16,185,129,0.15)]'
              : 'bg-rose-950/60 border-rose-500/60 text-rose-400 animate-pulse shadow-[0_0_12px_rgba(244,63,94,0.2)]'
          }`}
        >
          <span
            className={`w-2 h-2 rounded-full ${
              daemonOnline
                ? 'bg-emerald-400 shadow-[0_0_8px_#34d399] animate-pulse'
                : 'bg-rose-500 shadow-[0_0_8px_#f43f5e]'
            }`}
          />
          {daemonOnline ? (
            <span>● DAEMON ONLINE [127.0.0.1:8081]</span>
          ) : (
            <span>🔴 DAEMON DISCONNECTED</span>
          )}
        </div>

        {/* Node ID Badge */}
        <button
          onClick={handleCopyNode}
          title={`Node ID: ${rawNodeId} (Click to copy)`}
          className="flex items-center gap-1.5 px-2.5 py-1 rounded-xs bg-zinc-900 border border-zinc-800 hover:border-zinc-700 text-zinc-300 transition-colors font-mono text-[11px]"
        >
          <Server className="w-3.5 h-3.5 text-zinc-500" />
          <span className="text-zinc-400 font-sans text-[10px] uppercase tracking-wider font-bold">NODE:</span>
          <span className="text-cyan-400 font-bold">{truncatedNodeId}</span>
          {copiedNode ? (
            <Check className="w-3 h-3 text-emerald-400 ml-0.5" />
          ) : (
            <Copy className="w-3 h-3 text-zinc-500 opacity-60 hover:opacity-100 ml-0.5" />
          )}
        </button>
      </div>

      {/* Center/Right: Velocity Meter, Latest TxID, Pause Lever */}
      <div className="flex items-center gap-2.5">
        {/* Velocity Meter */}
        <div className="flex items-center gap-1.5 px-2.5 py-1 rounded-xs bg-zinc-900 border border-zinc-800 font-mono text-[11px]">
          <Activity className="w-3.5 h-3.5 text-cyan-400 animate-pulse" />
          <span className="text-zinc-400 font-sans text-[10px] uppercase tracking-wider font-bold">VELOCITY:</span>
          <span className="text-white font-bold tracking-tight">
            {currentTps.toLocaleString(undefined, { minimumFractionDigits: 0, maximumFractionDigits: 1 })}
          </span>
          <span className="text-cyan-400 text-[10px]">TPS</span>
        </div>

        {/* Latest TxID Badge */}
        <button
          onClick={handleCopyTx}
          title={`Latest Committed Transaction: ${displayTxHex} (Click to copy)`}
          className="flex items-center gap-1.5 px-2.5 py-1 rounded-xs bg-zinc-900 border border-zinc-800 hover:border-cyan-500/40 text-zinc-300 transition-colors font-mono text-[11px] group"
        >
          <Radio className="w-3.5 h-3.5 text-cyan-400" />
          <span className="text-zinc-400 font-sans text-[10px] uppercase tracking-wider font-bold">LATEST TX:</span>
          <span className="text-cyan-400 font-bold group-hover:text-cyan-300">
            {truncatedTx}
          </span>
          {copiedTx ? (
            <Check className="w-3 h-3 text-emerald-400 ml-0.5" />
          ) : (
            <Copy className="w-3 h-3 text-zinc-500 opacity-60 group-hover:opacity-100 ml-0.5" />
          )}
        </button>

        {/* Ingress Pause / Resume Pill */}
        <button
          onClick={handleToggleIngress}
          disabled={isToggling}
          className={`flex items-center gap-1.5 px-3 py-1 rounded-xs border font-mono text-[11px] uppercase tracking-wider font-bold transition-all cursor-pointer ${
            ingressPaused
              ? 'bg-amber-950/80 border-amber-500/80 text-amber-300 shadow-[0_0_12px_rgba(245,158,11,0.25)] animate-pulse'
              : 'bg-emerald-500/10 border-emerald-500/30 text-emerald-400 hover:bg-emerald-500/20 shadow-[0_0_8px_rgba(16,185,129,0.1)]'
          }`}
        >
          {ingressPaused ? (
            <>
              <Pause className="w-3.5 h-3.5 text-amber-400 fill-amber-400" />
              <span>[⏸ INGRESS PAUSED]</span>
            </>
          ) : (
            <>
              <Play className="w-3.5 h-3.5 text-emerald-400 fill-emerald-400" />
              <span>[▶ INGRESS ACTIVE]</span>
            </>
          )}
        </button>
      </div>
    </header>
  );
}
