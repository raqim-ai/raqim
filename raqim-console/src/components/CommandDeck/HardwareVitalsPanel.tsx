'use client';

import React, { useState, useEffect } from 'react';
import Link from 'next/link';
import { useSwarmStore } from '../../lib/store/useSwarmStore';
import { useHardwareVitals } from '../../lib/hooks/useHardwareVitals';
import { AreaChart, Area, XAxis, YAxis, ResponsiveContainer, Tooltip } from 'recharts';
import { Cpu, HardDrive, Shield, Activity, ArrowUpRight, Flame, Layers } from 'lucide-react';

export function HardwareVitalsPanel() {
  const vitalsHistory = useSwarmStore((state) => state.vitalsHistory);
  const clusterInfo = useSwarmStore((state) => state.clusterInfo);
  const quarantinedAgents = useSwarmStore((state) => state.quarantinedAgents);
  const aegisAlerts = useSwarmStore((state) => state.aegisAlerts);

  const [isMounted, setIsMounted] = useState(false);

  useEffect(() => {
    setIsMounted(true);
  }, []);

  const vitals = useHardwareVitals();

  // Dynamic process & host memory parsing
  const processMemoryMb = vitals?.process_memory_mb ?? vitals?.wasm_memory_mb ?? 0;
  const hostTotalMb = vitals?.host_total_memory_mb ?? 24576;
  const hostTotalGb = (hostTotalMb / 1024).toFixed(1);
  const memoryPercent = Math.min((processMemoryMb / hostTotalMb) * 100, 100);

  const walBytes = clusterInfo?.wal_bytes ?? 0;
  const walMb = (walBytes / (1024 * 1024)).toFixed(1);
  const bufferLoad = clusterInfo?.buffer_load ?? 0;

  const totalQuarantined = quarantinedAgents.length;
  const hasInterdictions = totalQuarantined > 0 || aegisAlerts.length > 0;

  return (
    <aside className="flex flex-col gap-3 h-full overflow-y-auto select-none scrollbar-thin scrollbar-thumb-zinc-800">
      {/* ── 1. CPU Load Area Chart (60s) ── */}
      <div className="bg-zinc-950/80 border border-zinc-800/80 rounded-sm p-3 flex flex-col justify-between shadow-sm">
        <div className="flex items-center justify-between text-zinc-400 mb-1">
          <div className="flex items-center gap-1.5 font-sans text-xs uppercase tracking-wider font-semibold">
            <Cpu className="w-3.5 h-3.5 text-cyan-400" />
            <span>CPU Allocation (60s)</span>
          </div>
          <span className="font-mono text-xs font-bold text-cyan-400">
            {vitals ? `${vitals.cpu_percent.toFixed(1)}%` : '--%'}
          </span>
        </div>

        <div className="w-full h-[84px] min-w-0 relative mt-1">
          {isMounted ? (
            <ResponsiveContainer width="100%" height="100%" minWidth={0} minHeight={70}>
              <AreaChart data={vitalsHistory}>
                <defs>
                  <linearGradient id="cpuGradient" x1="0" y1="0" x2="0" y2="1">
                    <stop offset="5%" stopColor="#06b6d4" stopOpacity={0.35} />
                    <stop offset="95%" stopColor="#06b6d4" stopOpacity={0.0} />
                  </linearGradient>
                </defs>
                <XAxis dataKey="time" hide />
                <YAxis domain={[0, 100]} hide />
                <Tooltip
                  contentStyle={{
                    backgroundColor: '#09090b',
                    borderColor: '#27272a',
                    borderRadius: '2px',
                    fontFamily: 'monospace',
                    fontSize: '11px',
                  }}
                  itemStyle={{ color: '#00f3ff' }}
                  labelStyle={{ color: '#a1a1aa' }}
                />
                <Area
                  type="monotone"
                  dataKey="cpu_load_percent"
                  name="CPU Load"
                  stroke="#06b6d4"
                  strokeWidth={1.5}
                  fillOpacity={1}
                  fill="url(#cpuGradient)"
                  isAnimationActive={false}
                />
              </AreaChart>
            </ResponsiveContainer>
          ) : (
            <div className="w-full h-full flex items-center justify-center bg-zinc-900/40 rounded-xs border border-zinc-800 font-mono text-[9px] text-zinc-500">
              [ SAMPLING KERNEL TELEMETRY ]
            </div>
          )}
        </div>
      </div>

      {/* ── 2. Process Memory Meter & Host Total Ceiling ── */}
      <div className="bg-zinc-950/80 border border-zinc-800/80 rounded-sm p-3 flex flex-col gap-2 shadow-sm">
        <div className="flex items-center justify-between text-zinc-400">
          <div className="flex items-center gap-1.5 font-sans text-xs uppercase tracking-wider font-semibold">
            <Activity className="w-3.5 h-3.5 text-indigo-400" />
            <span>Process Memory Meter</span>
          </div>
          <span className="font-mono text-xs text-indigo-300 font-bold">
            {processMemoryMb.toFixed(0)} MB
          </span>
        </div>

        <div className="space-y-1.5 my-auto">
          <div className="flex items-center justify-between font-mono text-[10px] text-zinc-400">
            <span>PROCESS RSS: {processMemoryMb.toFixed(0)} MB</span>
            <span>HOST CEILING: {hostTotalGb} GB</span>
          </div>
          <div className="w-full h-2 bg-zinc-900 rounded-xs overflow-hidden border border-zinc-800">
            <div
              className="h-full bg-gradient-to-r from-indigo-500 to-cyan-400 shadow-[0_0_8px_#6366f1] transition-all duration-300"
              style={{ width: `${Math.max(memoryPercent, 1.5)}%` }}
            />
          </div>
        </div>

        <div className="flex items-center justify-between pt-1 border-t border-zinc-800/80 font-mono text-[10px] text-zinc-500">
          <span>HOST MEMORY FOOTPRINT</span>
          <span className="text-indigo-400 font-semibold">{memoryPercent.toFixed(1)}%</span>
        </div>
      </div>

      {/* ── 3. WAL NVMe Footprint ── */}
      <div className="bg-zinc-950/80 border border-zinc-800/80 rounded-sm p-3 flex flex-col justify-between shadow-sm">
        <div className="flex items-center justify-between text-zinc-400">
          <div className="flex items-center gap-1.5 font-sans text-xs uppercase tracking-wider font-semibold">
            <HardDrive className="w-3.5 h-3.5 text-emerald-400" />
            <span>WAL NVMe Footprint</span>
          </div>
          <span className="font-mono text-[10px] text-emerald-400 font-bold uppercase tracking-wider">
            GROUP COMMIT
          </span>
        </div>

        <div className="my-1.5 flex items-baseline justify-between">
          <span className="font-mono text-xl font-bold text-white tracking-tight">
            {walMb} <span className="text-xs font-normal text-zinc-400">MB</span>
          </span>
          <span className="font-mono text-xs text-emerald-400 font-bold">
            {bufferLoad} BUFFER FRAMES
          </span>
        </div>

        <div className="flex items-center justify-between pt-1.5 border-t border-zinc-800/80 font-mono text-[10px] text-zinc-400">
          <span>ZERO-COPY RING</span>
          <span className="text-emerald-400 font-semibold">O_DIRECT SYNC</span>
        </div>
      </div>

      {/* ── 4. Aegis Security Perimeter Status ── */}
      <div className="bg-zinc-950/80 border border-zinc-800/80 rounded-sm p-3 flex flex-col justify-between shadow-sm">
        <div className="flex items-center justify-between text-zinc-400">
          <div className="flex items-center gap-1.5 font-sans text-xs uppercase tracking-wider font-semibold">
            <Shield className="w-3.5 h-3.5 text-cyan-400" />
            <span>Aegis Security Perimeter</span>
          </div>
          <Link
            href="/aegis"
            className="flex items-center gap-0.5 text-cyan-400 hover:text-cyan-300 font-mono text-[10px] uppercase font-bold"
          >
            <span>SOC</span>
            <ArrowUpRight className="w-3 h-3" />
          </Link>
        </div>

        <div className="my-1.5">
          <span
            className={`font-mono text-xs font-bold px-2 py-0.5 rounded-xs border inline-flex items-center gap-1.5 ${
              hasInterdictions
                ? 'bg-rose-950/80 text-rose-400 border-rose-800'
                : 'bg-emerald-500/10 text-emerald-400 border-emerald-500/30'
            }`}
          >
            {hasInterdictions ? (
              <>
                <Flame className="w-3 h-3 text-rose-400" />
                <span>INTERDICTIONS DETECTED ({totalQuarantined})</span>
              </>
            ) : (
              <>
                <span className="w-1.5 h-1.5 rounded-full bg-emerald-400 animate-pulse" />
                <span>ALL GATES SECURE</span>
              </>
            )}
          </span>
        </div>

        <div className="flex items-center justify-between pt-1.5 border-t border-zinc-800/80 font-mono text-[10px] text-zinc-400">
          <span>QUARANTINE JAIL</span>
          <span className={totalQuarantined > 0 ? 'text-rose-400 font-bold' : 'text-zinc-400'}>
            {totalQuarantined} ENCLAVES
          </span>
        </div>
      </div>
    </aside>
  );
}
