'use client';

import React from 'react';
import { VaultTelemetry } from '../../lib/api';
import { Database, HardDrive, Zap, Layers, Loader2 } from 'lucide-react';

interface VaultTelemetryRibbonProps {
  telemetry: VaultTelemetry | null;
  onTriggerCompaction?: () => Promise<void>;
  isCompacting?: boolean;
}

export function VaultTelemetryRibbon({
  telemetry,
  onTriggerCompaction,
  isCompacting = false,
}: VaultTelemetryRibbonProps) {
  const totalVectors = telemetry?.indexed_vectors ?? telemetry?.total_vectors ?? 0;
  const indexSizeMb = telemetry?.cold_storage_size_mb ?? telemetry?.index_size_mb ?? 0;
  const walPending = telemetry?.hot_wal_buffer_count ?? telemetry?.wal_pending_count ?? 0;
  const densestPartition =
    telemetry?.densest_partition ?? telemetry?.densest_namespace ?? 'Empty (0%)';
  const embedderName = telemetry?.embedder_name || 'BGE-Base';
  const embedderDims = telemetry?.embeder_dim ?? telemetry?.embedder_dims ?? 768;

  return (
    <div className="flex flex-col gap-3 w-full shrink-0 select-none">
      {/* Top Action Bar */}
      <div className="flex items-center justify-between gap-3 bg-zinc-950/60 border border-zinc-800/80 rounded-sm px-3.5 py-2">
        <div className="flex items-center gap-2 font-mono text-xs text-zinc-400">
          <Database className="w-4 h-4 text-cyan-400" />
          <span className="font-sans text-xs uppercase tracking-wider font-bold text-white">
            Forensic Audit Vault &amp; LanceDB Vector Telemetry
          </span>
        </div>

        {/* Trigger WAL Compaction Action Button */}
        {onTriggerCompaction && (
          <button
            onClick={onTriggerCompaction}
            disabled={isCompacting}
            className="flex items-center gap-2 px-3 py-1.5 rounded-xs border border-cyan-500/30 text-cyan-400 bg-cyan-950/20 hover:bg-cyan-900/40 font-mono text-xs font-bold uppercase transition-all shadow-[0_0_12px_rgba(0,243,255,0.15)] hover:shadow-[0_0_20px_rgba(0,243,255,0.3)] disabled:opacity-50 disabled:cursor-not-allowed cursor-pointer"
          >
            {isCompacting ? (
              <>
                <Loader2 className="w-3.5 h-3.5 animate-spin text-cyan-400" />
                <span>[⏳ ROTATING &amp; COMPACTING...]</span>
              </>
            ) : (
              <>
                <Zap className="w-3.5 h-3.5 text-cyan-400" />
                <span>[⚡ TRIGGER WAL COMPACTION]</span>
              </>
            )}
          </button>
        )}
      </div>

      {/* 4 Telemetry Metric Cards */}
      <section className="grid grid-cols-1 sm:grid-cols-2 lg:grid-cols-4 gap-3 w-full">
        {/* 1. Indexed Vectors Card */}
        <div className="bg-zinc-950/80 border border-zinc-800/80 rounded-sm p-3 flex flex-col justify-between relative overflow-hidden group hover:border-zinc-700 transition-colors">
          <div className="flex items-center justify-between text-zinc-400 mb-1.5">
            <div className="flex items-center gap-1.5 font-sans text-xs uppercase tracking-wider font-semibold">
              <Database className="w-3.5 h-3.5 text-cyan-400" />
              <span>Indexed Vectors</span>
            </div>
            <span className="font-mono text-[10px] text-cyan-400 font-bold">EMBEDDINGS</span>
          </div>

          <div className="my-1">
            <span className="font-mono text-xl font-bold text-cyan-400 tracking-tight">
              {totalVectors.toLocaleString()}
            </span>
          </div>

          <div className="flex items-center justify-between pt-1.5 border-t border-zinc-800/80 font-mono text-[10px] text-zinc-400">
            <span>FastEmbed {embedderName}</span>
            <span className="px-1.5 py-0.2 rounded-xs bg-cyan-500/10 border border-cyan-500/30 text-cyan-400 font-bold">
              {embedderDims}-DIM
            </span>
          </div>
        </div>

        {/* 2. Cold Storage Footprint Card */}
        <div className="bg-zinc-950/80 border border-zinc-800/80 rounded-sm p-3 flex flex-col justify-between relative overflow-hidden group hover:border-zinc-700 transition-colors">
          <div className="flex items-center justify-between text-zinc-400 mb-1.5">
            <div className="flex items-center gap-1.5 font-sans text-xs uppercase tracking-wider font-semibold">
              <HardDrive className="w-3.5 h-3.5 text-emerald-400" />
              <span>Cold Storage Size</span>
            </div>
            <span className="font-mono text-[10px] text-emerald-400 font-bold">PARQUET</span>
          </div>

          <div className="my-1 flex items-baseline justify-between">
            <span className="font-mono text-xl font-bold text-emerald-400 tracking-tight">
              {indexSizeMb.toFixed(2)} MB
            </span>
            <span className="px-1.5 py-0.5 rounded-xs bg-zinc-900 border border-zinc-800 font-mono text-[9px] text-emerald-400 font-bold">
              LANCEDB
            </span>
          </div>

          <div className="flex items-center justify-between pt-1.5 border-t border-zinc-800/80 font-mono text-[10px] text-zinc-400">
            <span>ON-DISK SEGMENTS</span>
            <span className="text-emerald-300 font-medium">OPTIMIZED</span>
          </div>
        </div>

        {/* 3. Pending Hot WAL Buffer Card */}
        <div className="bg-zinc-950/80 border border-zinc-800/80 rounded-sm p-3 flex flex-col justify-between relative overflow-hidden group hover:border-zinc-700 transition-colors">
          <div className="flex items-center justify-between text-zinc-400 mb-1.5">
            <div className="flex items-center gap-1.5 font-sans text-xs uppercase tracking-wider font-semibold">
              <Zap className="w-3.5 h-3.5 text-amber-400" />
              <span>Hot WAL Buffer</span>
            </div>
            <span className="font-mono text-[10px] text-amber-400 font-bold">IN-MEMORY</span>
          </div>

          <div className="my-1">
            <span className="font-mono text-xl font-bold text-amber-400 tracking-tight">
              {walPending.toLocaleString()}
            </span>
          </div>

          <div className="flex items-center justify-between pt-1.5 border-t border-zinc-800/80 font-mono text-[10px] text-zinc-400">
            <span>UNCOMPACTED FRAMES</span>
            <span className={walPending > 0 ? 'text-amber-400 font-bold' : 'text-emerald-400 font-medium'}>
              {walPending > 0 ? 'SYNCING TO DISK' : 'CLEAN'}
            </span>
          </div>
        </div>

        {/* 4. Densest Partition Card */}
        <div className="bg-zinc-950/80 border border-zinc-800/80 rounded-sm p-3 flex flex-col justify-between relative overflow-hidden group hover:border-zinc-700 transition-colors">
          <div className="flex items-center justify-between text-zinc-400 mb-1.5">
            <div className="flex items-center gap-1.5 font-sans text-xs uppercase tracking-wider font-semibold">
              <Layers className="w-3.5 h-3.5 text-purple-400" />
              <span>Densest Partition</span>
            </div>
            <span className="font-mono text-[10px] text-purple-400 font-bold">CONCENTRATION</span>
          </div>

          <div className="my-1">
            <span
              className="font-mono text-sm font-bold text-purple-300 tracking-tight truncate block"
              title={densestPartition}
            >
              {densestPartition}
            </span>
          </div>

          <div className="flex items-center justify-between pt-1.5 border-t border-zinc-800/80 font-mono text-[10px] text-zinc-400">
            <span>ACTIVE INDEX REGION</span>
            <span className="text-purple-300 font-medium">SHARD #0</span>
          </div>
        </div>
      </section>
    </div>
  );
}
