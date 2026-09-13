'use client';

import React, { useState, useEffect } from 'react';
import Link from 'next/link';
import { usePathname } from 'next/navigation';
import { motion } from 'framer-motion';
import { useSwarmStore } from '../../lib/store/useSwarmStore';
import { fetchClusterDiagnostics } from '../../actions/admin';
import {
  History,
  LayoutDashboard,
  Network,
  Shield,
  Vault,
  Server,
  Copy,
  Check,
} from 'lucide-react';

export function Sidebar() {
  const pathname = usePathname();
  const daemonOnline = useSwarmStore((state) => state.daemonOnline);
  const currentTps = useSwarmStore((state) => state.currentTps);
  const quarantinedAgents = useSwarmStore((state) => state.quarantinedAgents);
  const activeTopology = useSwarmStore((state) => state.activeTopology);

  const [clusterInfo, setClusterInfo] = useState<{
    node_id: string;
    wal_bytes: number;
    buffer_load: number;
  } | null>(null);

  const [copied, setCopied] = useState(false);

  useEffect(() => {
    fetchClusterDiagnostics().then((data) => {
      if (data) {
        setClusterInfo(data);
      }
    });
  }, [daemonOnline]);

  const rawNodeId = clusterInfo?.node_id || 'node_01_alpha';
  const displayNodeId =
    rawNodeId.length > 18
      ? `${rawNodeId.slice(0, 8)}...${rawNodeId.slice(-4)}`
      : rawNodeId;

  const handleCopyNodeId = async () => {
    if (!rawNodeId) return;
    try {
      await navigator.clipboard.writeText(rawNodeId);
      setCopied(true);
      setTimeout(() => setCopied(false), 2000);
    } catch {
      // Ignore clipboard write failure
    }
  };

  const navLinks = [
    { href: '/', label: 'Dashboard', icon: LayoutDashboard },
    { href: '/topology', label: 'Topology', icon: Network },
    { href: '/aegis', label: 'Aegis Governance', icon: Shield },
    { href: '/vault', label: 'Audit Vault', icon: Vault },
    { href: '/replay', label: 'Time Travel // Replay', icon: History },
  ];

  return (
    <aside className="w-64 h-full border-r border-zinc-800/80 bg-zinc-950 flex flex-col shrink-0 z-40 select-none">
      {/* ── Brand & Architectural Monogram ── */}
      <div className="p-4 border-b border-zinc-800/80 bg-zinc-950 flex items-center gap-3 shrink-0">
        <svg
          viewBox="0 0 32 32"
          fill="none"
          className="w-7 h-7 text-white shrink-0"
          xmlns="http://www.w3.org/2000/svg"
        >
          {/* Architectural R: Pure, solid, authoritative */}
          <path
            d="M5 4H18.5C23.2 4 26.5 7.2 26.5 12C26.5 16.1 23.8 19 19.8 19.8L27 28H21.2L14.7 20H10.5V28H5V4ZM10.5 8.8V15.5H18C20.6 15.5 22 14.2 22 12C22 9.8 20.6 8.8 18 8.8H10.5Z"
            fill="currentColor"
          />
        </svg>

        {/* Brand Typography */}
        <div className="flex flex-col min-w-0">
          <div className="flex items-center gap-2">
            <span className="font-headline font-black text-sm tracking-[0.22em] text-white uppercase leading-none">
              RAQIM
            </span>
            <span className="font-mono text-[9px] font-medium tracking-widest px-1.5 py-0.5 rounded bg-zinc-900 text-zinc-400 border border-zinc-800 leading-none">
              CORE
            </span>
          </div>
          <span className="font-mono text-[9px] tracking-[0.14em] text-zinc-500 uppercase mt-1 leading-none">
            SOVEREIGN DATA PLANE
          </span>
        </div>
      </div>

      {/* ── Refined Control Plane / Node Status Card ── */}
      <div className="p-3 border-b border-zinc-800/80 bg-zinc-900/30 shrink-0">
        <div className="p-2.5 rounded-lg bg-zinc-900/80 border border-zinc-800/80 shadow-sm transition-all hover:border-zinc-700/60">
          {/* Header Row: Label + Live Status Badge */}
          <div className="flex items-center justify-between gap-2 mb-2">
            <div className="flex items-center gap-1.5">
              <Server className="w-3.5 h-3.5 text-zinc-400 shrink-0" />
              <span className="font-mono text-[9px] font-semibold text-zinc-400 uppercase tracking-widest">
                CONTROL PLANE
              </span>
            </div>

            {/* Live Status Badge */}
            <div
              className={`inline-flex items-center gap-1.5 px-2 py-0.5 rounded-full border text-[9px] font-mono font-bold tracking-wider ${
                daemonOnline
                  ? 'bg-emerald-500/10 border-emerald-500/30 text-emerald-400'
                  : 'bg-rose-500/10 border-rose-500/30 text-rose-400'
              }`}
            >
              <span className="relative flex h-1.5 w-1.5">
                {daemonOnline && (
                  <span className="animate-ping absolute inline-flex h-full w-full rounded-full bg-emerald-400 opacity-75" />
                )}
                <span
                  className={`relative inline-flex rounded-full h-1.5 w-1.5 ${
                    daemonOnline ? 'bg-emerald-400' : 'bg-rose-400'
                  }`}
                />
              </span>
              <span>{daemonOnline ? 'ONLINE' : 'OFFLINE'}</span>
            </div>
          </div>

          {/* Node ID Row with One-Click Copy */}
          <div className="flex items-center justify-between gap-1.5 bg-zinc-950/70 rounded px-2 py-1.5 border border-zinc-800/60">
            <div className="flex flex-col min-w-0">
              <span className="font-mono text-[8px] text-zinc-500 uppercase tracking-wider leading-none mb-1">
                ACTIVE NODE
              </span>
              <span
                className="font-mono text-[11px] font-semibold text-zinc-200 truncate leading-none"
                title={rawNodeId}
              >
                {displayNodeId}
              </span>
            </div>
            <button
              onClick={handleCopyNodeId}
              className="shrink-0 p-1 text-zinc-400 hover:text-zinc-200 hover:bg-zinc-800/60 rounded transition-colors"
              title="Copy Node ID"
            >
              {copied ? (
                <Check className="w-3 h-3 text-emerald-400" />
              ) : (
                <Copy className="w-3 h-3" />
              )}
            </button>
          </div>

          {/* Transport Details Footer */}
          <div className="flex items-center justify-between mt-2 pt-2 border-t border-zinc-800/50 font-mono text-[9px] text-zinc-500">
            <span className="tracking-wider">127.0.0.1:8081</span>
            <span className="text-[8px] font-semibold px-1 py-0.5 rounded bg-zinc-800 text-zinc-400 tracking-wider">
              ZERO-TRUST
            </span>
          </div>
        </div>
      </div>

      {/* ── Navigation Links (Preserved Motion & Look) ── */}
      <nav className="flex flex-col gap-1 p-3 flex-1 overflow-y-auto scrollbar-thin scrollbar-thumb-zinc-800 scrollbar-track-transparent">
        {navLinks.map((link) => {
          const isActive =
            pathname === link.href ||
            (link.href === '/aegis' && pathname === '/firewall') ||
            (link.href === '/vault' && pathname === '/audit-vault') ||
            (link.href === '/replay' && pathname === '/router');
          const Icon = link.icon;

          return (
            <Link
              key={link.href}
              href={link.href}
              className={`relative flex items-center gap-3 px-3 py-2.5 rounded-md font-mono text-xs uppercase tracking-wider transition-all duration-150 ${
                isActive
                  ? 'text-white font-semibold bg-zinc-900/60'
                  : 'text-zinc-400 hover:text-zinc-200 hover:bg-zinc-900/30 font-normal'
              }`}
            >
              {isActive && (
                <motion.div
                  layoutId="activeNavLine"
                  transition={{ type: 'spring', stiffness: 400, damping: 30 }}
                  className="absolute left-0 top-1.5 bottom-1.5 w-0.5 bg-cyan-400 shadow-[0_0_8px_rgba(34,211,238,0.8)] rounded-r"
                />
              )}
              <Icon
                className={`w-4 h-4 shrink-0 transition-colors ${
                  isActive ? 'text-cyan-400' : 'text-zinc-500 group-hover:text-zinc-300'
                }`}
              />
              <span className="truncate">{link.label}</span>
            </Link>
          );
        })}
      </nav>

      {/* ── Diagnostics Footer ── */}
      <div className="p-4 border-t border-zinc-800/80 bg-zinc-950 shrink-0">
        <div className="flex flex-col gap-2">
          <div className="flex justify-between items-center font-mono text-[9px]">
            <span className="text-zinc-500 uppercase tracking-widest">DAEMON</span>
            <span
              className={`font-semibold tracking-wider ${
                daemonOnline ? 'text-emerald-400' : 'text-rose-500'
              }`}
            >
              {daemonOnline ? 'CONNECTED' : 'DISCONNECTED'}
            </span>
          </div>
          <div className="flex justify-between items-center font-mono text-[9px]">
            <span className="text-zinc-500 uppercase tracking-widest">THROUGHPUT</span>
            <span className="font-semibold text-zinc-300 tracking-wider">
              {currentTps} TPS
            </span>
          </div>
          <div className="flex justify-between items-center font-mono text-[9px]">
            <span className="text-zinc-500 uppercase tracking-widest">SHARDS</span>
            <span className="font-semibold text-zinc-300 tracking-wider">
              {activeTopology.length} ACTIVE
            </span>
          </div>
          <div className="flex justify-between items-center font-mono text-[9px]">
            <span className="text-zinc-500 uppercase tracking-widest">QUARANTINE</span>
            <span
              className={`font-semibold tracking-wider ${
                quarantinedAgents.length > 0 ? 'text-rose-500' : 'text-zinc-300'
              }`}
            >
              {quarantinedAgents.length} BLOCKED
            </span>
          </div>
        </div>
      </div>
    </aside>
  );
}
