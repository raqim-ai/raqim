'use client';

import React, { useMemo } from 'react';
import Link from 'next/link';
import { useSwarmStore, DashboardCardsData, VaultTelemetry } from '../../lib/store/useSwarmStore';
import { Database, Bot, Layers, Zap, ArrowUpRight } from 'lucide-react';

interface MetricCardsGridProps {
  initialCards?: DashboardCardsData | null;
  initialVaultTelemetry?: VaultTelemetry | null;
}

export function MetricCardsGrid({
  initialCards,
  initialVaultTelemetry,
}: MetricCardsGridProps) {
  const storeDashboardCards = useSwarmStore((state) => state.dashboardCards);
  const storeVaultTelemetry = useSwarmStore((state) => state.vaultTelemetry);
  const currentTps = useSwarmStore((state) => state.currentTps);
  const tpsHistory = useSwarmStore((state) => state.tpsHistory);
  const agentLastSeen = useSwarmStore((state) => state.agentLastSeen);
  const highestTxId = useSwarmStore((state) => state.highestTxId);

  // Reconcile live store values with server pre-rendered values
  const cards = storeDashboardCards || initialCards;
  const vault = storeVaultTelemetry || initialVaultTelemetry;

  const coldCount = cards?.cold_thoughts_count ?? vault?.total_vectors ?? cards?.vault_capacity ?? 0;
  const hotCount = cards?.hot_thoughts_count ?? vault?.wal_pending_count ?? 0;
  const globalTransactions =
    cards?.global_transactions ??
    (cards?.global_transactions === 0 ? 0 : Math.max(highestTxId, coldCount + hotCount));

  const activeAgentsCount = Math.max(
    cards?.active_agents ?? 0,
    Object.keys(agentLastSeen).length
  );

  const vaultCapacity = cards?.vault_capacity ?? vault?.total_vectors ?? coldCount;
  const embedderName = cards?.embedder_name || 'BGE-Small';
  const embedderDims = cards?.embedder_dims || 384;

  // Build SVG sparkline path from last 30 data points of TPS history
  const sparklineData = useMemo(() => {
    const points = tpsHistory.slice(-30);
    if (points.length < 2) return '';

    const width = 120;
    const height = 28;
    const maxTps = Math.max(...points.map((p) => p.tps), 10);
    const minTps = 0;

    const coords = points.map((pt, idx) => {
      const x = (idx / (points.length - 1)) * width;
      const y = height - ((pt.tps - minTps) / (maxTps - minTps)) * (height - 4) - 2;
      return `${x.toFixed(1)},${y.toFixed(1)}`;
    });

    return coords.join(' ');
  }, [tpsHistory]);

  return (
    <section className="grid grid-cols-1 sm:grid-cols-2 lg:grid-cols-4 gap-3 w-full shrink-0">
      {/* CARD 1: Lifetime Ingestion */}
      <div className="bg-zinc-950/80 border border-zinc-800/80 rounded-sm p-3 flex flex-col justify-between relative overflow-hidden group hover:border-zinc-700 transition-colors">
        <div className="flex items-center justify-between text-zinc-400 mb-1.5">
          <div className="flex items-center gap-1.5 font-sans text-xs uppercase tracking-wider font-semibold">
            <Database className="w-3.5 h-3.5 text-cyan-400" />
            <span>Lifetime Ingestion</span>
          </div>
          <span className="font-mono text-[10px] text-zinc-500 font-bold">TOTAL TX</span>
        </div>

        <div className="my-1">
          <span className="font-mono text-xl font-bold text-cyan-400 tracking-tight">
            {globalTransactions.toLocaleString()}
          </span>
        </div>

        <div className="flex items-center gap-2 pt-1.5 border-t border-zinc-800/80 font-mono text-[10px] text-zinc-400 truncate">
          <span className="text-cyan-400/90 font-medium">
            [COLD: {coldCount.toLocaleString()}]
          </span>
          <span className="text-zinc-600">|</span>
          <span className="text-amber-400/90 font-medium">
            [HOT: {hotCount.toLocaleString()}]
          </span>
        </div>
      </div>

      {/* CARD 2: Active Swarm Agents */}
      <div className="bg-zinc-950/80 border border-zinc-800/80 rounded-sm p-3 flex flex-col justify-between relative overflow-hidden group hover:border-zinc-700 transition-colors">
        <div className="flex items-center justify-between text-zinc-400 mb-1.5">
          <div className="flex items-center gap-1.5 font-sans text-xs uppercase tracking-wider font-semibold">
            <Bot className="w-3.5 h-3.5 text-emerald-400" />
            <span>Active Enclaves</span>
          </div>
          <span className="font-mono text-[10px] text-emerald-400/80 font-bold">60S WINDOW</span>
        </div>

        <div className="my-1 flex items-baseline justify-between">
          <span className="font-mono text-xl font-bold text-white tracking-tight">
            {activeAgentsCount.toLocaleString()}
          </span>
          <span className="font-mono text-xs text-zinc-500 font-medium">CONCURRENT</span>
        </div>

        <div className="pt-1.5 border-t border-zinc-800/80 font-mono text-[10px]">
          <Link
            href="/topology"
            className="text-emerald-400 hover:text-emerald-300 flex items-center justify-between group-hover:underline transition-colors"
          >
            <span>[Inspect Topology -&gt;]</span>
            <ArrowUpRight className="w-3 h-3" />
          </Link>
        </div>
      </div>

      {/* CARD 3: Memory Vector Vault Capacity */}
      <div className="bg-zinc-950/80 border border-zinc-800/80 rounded-sm p-3 flex flex-col justify-between relative overflow-hidden group hover:border-zinc-700 transition-colors">
        <div className="flex items-center justify-between text-zinc-400 mb-1.5">
          <div className="flex items-center gap-1.5 font-sans text-xs uppercase tracking-wider font-semibold">
            <Layers className="w-3.5 h-3.5 text-indigo-400" />
            <span>Vault Vectors</span>
          </div>
          <span className="font-mono text-[10px] text-zinc-500 font-bold">LANCEDB</span>
        </div>

        <div className="my-1">
          <div className="font-mono text-xl font-bold text-white tracking-tight">
            {vaultCapacity.toLocaleString()}
          </div>
        </div>

        <div className="flex items-center justify-between pt-1.5 border-t border-zinc-800/80 font-mono text-[10px] text-indigo-300/80 truncate">
          <span>FastEmbed {embedderName} ({embedderDims}-dim)</span>
        </div>
      </div>

      {/* CARD 4: Live Ingress Velocity (TPS) */}
      <div className="bg-zinc-950/80 border border-zinc-800/80 rounded-sm p-3 flex flex-col justify-between relative overflow-hidden group hover:border-zinc-700 transition-colors">
        <div className="flex items-center justify-between text-zinc-400 mb-1.5">
          <div className="flex items-center gap-1.5 font-sans text-xs uppercase tracking-wider font-semibold">
            <Zap className="w-3.5 h-3.5 text-amber-400" />
            <span>Ingress Velocity</span>
          </div>
          <span className="font-mono text-[10px] text-amber-400/80 font-bold">ROLLING 1S</span>
        </div>

        <div className="my-1 flex items-center justify-between gap-2">
          <div className="font-mono text-xl font-bold text-amber-400 tracking-tight flex items-baseline gap-1">
            <span>{currentTps.toLocaleString(undefined, { minimumFractionDigits: 0, maximumFractionDigits: 1 })}</span>
            <span className="text-xs text-zinc-500 font-normal">TPS</span>
          </div>

          {/* Micro SVG Sparkline (Last 30s) */}
          <div className="w-[110px] h-[26px] bg-zinc-900/90 border border-zinc-800 rounded-xs p-0.5 overflow-hidden flex items-center justify-center">
            {sparklineData ? (
              <svg viewBox="0 0 120 28" className="w-full h-full">
                <polyline
                  fill="none"
                  stroke="#fbbf24"
                  strokeWidth="1.5"
                  strokeLinecap="round"
                  strokeLinejoin="round"
                  points={sparklineData}
                />
              </svg>
            ) : (
              <span className="font-mono text-[9px] text-zinc-600 tracking-widest">---</span>
            )}
          </div>
        </div>

        <div className="flex items-center justify-between pt-1.5 border-t border-zinc-800/80 font-mono text-[10px] text-zinc-400">
          <span>PEAK: {Math.max(...tpsHistory.map((p) => p.tps), 0)} TPS</span>
          <span className="text-amber-400/80">30S SPARKLINE</span>
        </div>
      </div>
    </section>
  );
}
