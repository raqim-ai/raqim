'use client';

import React from 'react';
import { AegisMetricsData } from '../../lib/api';
import { ShieldAlert, Flame, KeyRound, Lock } from 'lucide-react';

interface AegisMetricsRibbonProps {
  metrics: AegisMetricsData | null;
  activeQuarantinedCount?: number;
}

export function AegisMetricsRibbon({ metrics, activeQuarantinedCount }: AegisMetricsRibbonProps) {
  const totalQuarantined = activeQuarantinedCount ?? metrics?.total_quarantined ?? 0;
  const recentInterdictions = metrics?.recent_interdictions ?? 0;
  const signatureSpoofs = metrics?.signarure_spoofs ?? metrics?.signature_spoofs ?? 0;
  const namespaceBreaches = metrics?.namespace_breaches ?? 0;

  return (
    <section className="grid grid-cols-1 sm:grid-cols-2 lg:grid-cols-4 gap-3 w-full shrink-0 select-none">
      {/* 1. Active Quarantined Agents */}
      <div className="bg-zinc-950/80 border border-zinc-800/80 rounded-sm p-3 flex flex-col justify-between relative overflow-hidden group hover:border-zinc-700 transition-colors">
        <div className="flex items-center justify-between text-zinc-400 mb-1.5">
          <div className="flex items-center gap-1.5 font-sans text-xs uppercase tracking-wider font-semibold">
            <ShieldAlert className="w-3.5 h-3.5 text-rose-400" />
            <span>Active Quarantined</span>
          </div>
          <span className="font-mono text-[10px] text-rose-400/80 font-bold">INTERDICTED</span>
        </div>

        <div className="my-1 flex items-baseline justify-between">
          <span className="font-mono text-xl font-bold text-white tracking-tight">
            {totalQuarantined.toLocaleString()}
          </span>
          <span className="font-mono text-xs text-rose-400 font-bold">ENCLAVES</span>
        </div>

        <div className="flex items-center justify-between pt-1.5 border-t border-zinc-800/80 font-mono text-[10px] text-zinc-400">
          <span>JAIL ISOLATION</span>
          <span className={totalQuarantined > 0 ? 'text-rose-400 font-bold' : 'text-emerald-400'}>
            {totalQuarantined > 0 ? 'ACTIVE INTERDICTION' : 'ALL GATES SECURE'}
          </span>
        </div>
      </div>

      {/* 2. Recent Interdictions (10m) */}
      <div className="bg-zinc-950/80 border border-zinc-800/80 rounded-sm p-3 flex flex-col justify-between relative overflow-hidden group hover:border-zinc-700 transition-colors">
        <div className="flex items-center justify-between text-zinc-400 mb-1.5">
          <div className="flex items-center gap-1.5 font-sans text-xs uppercase tracking-wider font-semibold">
            <Flame className="w-3.5 h-3.5 text-amber-400" />
            <span>Recent Interdictions</span>
          </div>
          <span className="font-mono text-[10px] text-amber-400/80 font-bold">10-MIN TALLY</span>
        </div>

        <div className="my-1 flex items-baseline justify-between">
          <span className="font-mono text-xl font-bold text-amber-400 tracking-tight">
            {recentInterdictions.toLocaleString()}
          </span>
          <span className="font-mono text-xs text-zinc-500 font-medium">DROPS</span>
        </div>

        <div className="flex items-center justify-between pt-1.5 border-t border-zinc-800/80 font-mono text-[10px] text-zinc-400">
          <span>THREAT FREQUENCY</span>
          <span className="text-amber-400/80 font-medium">KERNEL DROPS</span>
        </div>
      </div>

      {/* 3. Signature Spoof Blocks */}
      <div className="bg-zinc-950/80 border border-zinc-800/80 rounded-sm p-3 flex flex-col justify-between relative overflow-hidden group hover:border-zinc-700 transition-colors">
        <div className="flex items-center justify-between text-zinc-400 mb-1.5">
          <div className="flex items-center gap-1.5 font-sans text-xs uppercase tracking-wider font-semibold">
            <KeyRound className="w-3.5 h-3.5 text-purple-400" />
            <span>Signature Spoofs</span>
          </div>
          <span className="font-mono text-[10px] text-purple-400/80 font-bold">ED25519 VERIFY</span>
        </div>

        <div className="my-1 flex items-baseline justify-between">
          <span className="font-mono text-xl font-bold text-purple-400 tracking-tight">
            {signatureSpoofs.toLocaleString()}
          </span>
          <span className="font-mono text-xs text-zinc-500 font-medium">FORGERIES</span>
        </div>

        <div className="flex items-center justify-between pt-1.5 border-t border-zinc-800/80 font-mono text-[10px] text-zinc-400">
          <span>FORGED PASSPORTS</span>
          <span className="text-purple-300 font-medium">INTERCEPTED</span>
        </div>
      </div>

      {/* 4. Namespace Access Breaches */}
      <div className="bg-zinc-950/80 border border-zinc-800/80 rounded-sm p-3 flex flex-col justify-between relative overflow-hidden group hover:border-zinc-700 transition-colors">
        <div className="flex items-center justify-between text-zinc-400 mb-1.5">
          <div className="flex items-center gap-1.5 font-sans text-xs uppercase tracking-wider font-semibold">
            <Lock className="w-3.5 h-3.5 text-cyan-400" />
            <span>Namespace Breaches</span>
          </div>
          <span className="font-mono text-[10px] text-cyan-400/80 font-bold">BOUNDARY ACL</span>
        </div>

        <div className="my-1 flex items-baseline justify-between">
          <span className="font-mono text-xl font-bold text-cyan-400 tracking-tight">
            {namespaceBreaches.toLocaleString()}
          </span>
          <span className="font-mono text-xs text-zinc-500 font-medium">BLOCKED</span>
        </div>

        <div className="flex items-center justify-between pt-1.5 border-t border-zinc-800/80 font-mono text-[10px] text-zinc-400">
          <span>UNAUTHORIZED DOMAINS</span>
          <span className="text-cyan-300 font-medium">ENFORCED</span>
        </div>
      </div>
    </section>
  );
}
