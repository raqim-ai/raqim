'use client';

import React from 'react';
import { GroupPolicyTelemetry } from '../../lib/api';
import { Gauge, Lock, CheckCircle2 } from 'lucide-react';

interface TokenBucketGaugesProps {
  policies: GroupPolicyTelemetry[];
}

export function TokenBucketGauges({ policies }: TokenBucketGaugesProps) {
  const getCapacityColor = (pct: number) => {
    if (pct > 50) return {
      bar: 'bg-emerald-500 shadow-[0_0_8px_#10b981]',
      text: 'text-emerald-400',
      border: 'border-emerald-800/60',
    };
    if (pct >= 20) return {
      bar: 'bg-amber-500 shadow-[0_0_8px_#f59e0b]',
      text: 'text-amber-400',
      border: 'border-amber-800/60',
    };
    return {
      bar: 'bg-rose-500 shadow-[0_0_8px_#f43f5e]',
      text: 'text-rose-400 animate-pulse',
      border: 'border-rose-800/60',
    };
  };

  return (
    <div className="bg-zinc-950/80 border border-zinc-800/80 rounded-sm p-3.5 flex flex-col gap-3 shadow-lg select-none">
      {/* Header */}
      <div className="flex items-center justify-between text-zinc-400 pb-2 border-b border-zinc-800 select-none">
        <div className="flex items-center gap-1.5 font-sans text-xs uppercase tracking-wider font-bold text-zinc-200">
          <Gauge className="w-3.5 h-3.5 text-cyan-400" />
          <span>Token Bucket Rate-Limit Gauges</span>
        </div>
        <span className="font-mono text-[10px] text-cyan-400 font-bold">
          {policies.length} POLICIES ACTIVE
        </span>
      </div>

      {/* Policies List */}
      <div className="space-y-3.5 overflow-y-auto max-h-72 scrollbar-thin scrollbar-thumb-zinc-800">
        {policies.length === 0 ? (
          <div className="text-center py-6 text-zinc-500 font-mono text-[11px] uppercase tracking-wider">
            [ POLICIES SYNCED WITH DAEMON DEFAULTS ]
          </div>
        ) : (
          policies.map((policy) => {
            const burst = policy.burst_capacity || 1;
            const remaining = policy.remaining_tokens;
            const pct = Math.min(Math.max((remaining / burst) * 100, 0), 100);
            const color = getCapacityColor(pct);

            const allowed = policy.allowed_namespaces || policy.allowed_namspace || ['*'];
            const blocked = policy.blocked_namespaces || policy.blocked_namespace || [];

            return (
              <div
                key={policy.group_name}
                className={`bg-zinc-900 border ${color.border} rounded-xs p-3 space-y-2.5 transition-colors`}
              >
                {/* Group Name & Status */}
                <div className="flex items-center justify-between font-mono">
                  <span className="text-white font-bold text-xs tracking-tight">
                    {policy.group_name}
                  </span>
                  <span className={`text-[11px] font-bold ${color.text}`}>
                    {pct.toFixed(1)}% QUOTA
                  </span>
                </div>

                {/* Progress Bar */}
                <div className="space-y-1">
                  <div className="w-full h-2 bg-zinc-950 rounded-xs overflow-hidden border border-zinc-800">
                    <div
                      className={`h-full ${color.bar} transition-all duration-300`}
                      style={{ width: `${pct}%` }}
                    />
                  </div>
                  <div className="flex items-center justify-between font-mono text-[10px] text-zinc-400">
                    <span className={color.text}>
                      {remaining.toLocaleString()} / {burst.toLocaleString()} TOKENS
                    </span>
                    <span>MAX {policy.max_tps.toLocaleString()} TPS</span>
                  </div>
                </div>

                {/* Namespaces Badges */}
                <div className="flex flex-wrap items-center gap-1.5 pt-1 text-[10px] font-mono">
                  {/* Allowed */}
                  {allowed.map((ns) => (
                    <span
                      key={ns}
                      className="inline-flex items-center gap-1 px-1.5 py-0.5 rounded-xs bg-emerald-500/10 border border-emerald-500/30 text-emerald-400 font-bold"
                    >
                      <CheckCircle2 className="w-2.5 h-2.5" />
                      <span>{ns}</span>
                    </span>
                  ))}

                  {/* Blocked */}
                  {blocked.map((ns) => (
                    <span
                      key={ns}
                      className="inline-flex items-center gap-1 px-1.5 py-0.5 rounded-xs bg-rose-950/60 border border-rose-800/60 text-rose-400 font-bold"
                    >
                      <Lock className="w-2.5 h-2.5" />
                      <span>{ns}</span>
                    </span>
                  ))}
                </div>
              </div>
            );
          })
        )}
      </div>
    </div>
  );
}
