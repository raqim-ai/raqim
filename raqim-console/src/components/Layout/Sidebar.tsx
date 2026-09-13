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
      {/* ── Brand & Cryptographic Monogram Emblem ── */}
      <div className="p-4 border-b border-zinc-800/80 bg-zinc-950 flex items-center gap-3 shrink-0">
        <div className="relative w-10 h-10 shrink-0 rounded-lg bg-zinc-900/90 border border-zinc-700/60 p-1 flex items-center justify-center shadow-[0_0_15px_rgba(34,211,238,0.12)] ring-1 ring-white/5">
          <svg
            viewBox="0 0 40 40"
            fill="none"
            className="w-full h-full"
            xmlns="http://www.w3.org/2000/svg"
          >
            <defs>
              <linearGradient id="raqimStroke" x1="0%" y1="0%" x2="100%" y2="100%">
                <stop offset="0%" stopColor="#22d3ee" />
                <stop offset="50%" stopColor="#38bdf8" />
                <stop offset="100%" stopColor="#10b981" />
              </linearGradient>
              <linearGradient id="hexBorder" x1="0%" y1="0%" x2="100%" y2="100%">
                <stop offset="0%" stopColor="#38bdf8" stopOpacity="0.7" />
                <stop offset="100%" stopColor="#059669" stopOpacity="0.4" />
              </linearGradient>
              <linearGradient
                id="facetTop"
                x1="20"
                y1="3"
                x2="20"
                y2="20"
                gradientUnits="userSpaceOnUse"
              >
                <stop offset="0%" stopColor="#1e293b" stopOpacity="0.9" />
                <stop offset="100%" stopColor="#0f172a" stopOpacity="0.6" />
              </linearGradient>
              <linearGradient
                id="facetLeft"
                x1="5"
                y1="20"
                x2="20"
                y2="20"
                gradientUnits="userSpaceOnUse"
              >
                <stop offset="0%" stopColor="#090d14" stopOpacity="0.8" />
                <stop offset="100%" stopColor="#020617" stopOpacity="0.9" />
              </linearGradient>
              <linearGradient
                id="facetRight"
                x1="20"
                y1="20"
                x2="35"
                y2="20"
                gradientUnits="userSpaceOnUse"
              >
                <stop offset="0%" stopColor="#0f172a" stopOpacity="0.8" />
                <stop offset="100%" stopColor="#020617" stopOpacity="0.95" />
              </linearGradient>
              <radialGradient id="coreGlow" cx="50%" cy="50%" r="50%">
                <stop offset="0%" stopColor="#22d3ee" stopOpacity="0.25" />
                <stop offset="100%" stopColor="#22d3ee" stopOpacity="0" />
              </radialGradient>
            </defs>

            {/* Core Ambient Glow */}
            <circle cx="20" cy="20" r="14" fill="url(#coreGlow)" />

            {/* Outer Isometric Inscribed Tablet / Merkle Shield */}
            <path
              d="M20 3.5L35 11.5V28.5L20 36.5L5 28.5V11.5Z"
              stroke="url(#hexBorder)"
              strokeWidth="1.2"
              strokeLinejoin="round"
            />
            <path
              d="M20 3.5L35 11.5L20 20L5 11.5Z"
              fill="url(#facetTop)"
              stroke="#334155"
              strokeWidth="0.5"
            />
            <path
              d="M5 11.5L20 20V36.5L5 28.5Z"
              fill="url(#facetLeft)"
              stroke="#334155"
              strokeWidth="0.5"
            />
            <path
              d="M35 11.5L20 20V36.5L35 28.5Z"
              fill="url(#facetRight)"
              stroke="#334155"
              strokeWidth="0.5"
            />

            {/* Inscribed Merkle Monogram 'R' */}
            {/* Vertical Spine */}
            <path
              d="M13 10.5V29.5"
              stroke="url(#raqimStroke)"
              strokeWidth="2.2"
              strokeLinecap="round"
            />
            {/* Upper Vault Loop */}
            <path
              d="M13 10.5H21C24.5 10.5 26.5 12.5 26.5 15.5C26.5 18.5 24.5 20.5 21 20.5H13"
              stroke="url(#raqimStroke)"
              strokeWidth="2.2"
              strokeLinecap="round"
              strokeLinejoin="round"
            />
            {/* Lower Merkle DAG Branch Kick */}
            <path
              d="M18 20.5L26.5 29.5"
              stroke="url(#raqimStroke)"
              strokeWidth="2.2"
              strokeLinecap="round"
            />

            {/* Cryptographic Verification Nodes */}
            <circle cx="13" cy="10.5" r="1.6" fill="#22d3ee" />
            <circle cx="26.5" cy="15.5" r="1.6" fill="#38bdf8" />
            <circle cx="13" cy="20.5" r="1.6" fill="#22d3ee" />
            <circle cx="13" cy="29.5" r="1.6" fill="#10b981" />
            <circle cx="26.5" cy="29.5" r="1.6" fill="#34d399" />

            {/* Inscribed Core Seed Glyph */}
            <polygon points="19,14 21,15.5 19,17 17,15.5" fill="#38bdf8" />
          </svg>
        </div>

        {/* Brand Typography */}
        <div className="flex flex-col min-w-0">
          <div className="flex items-center gap-2">
            <span className="font-headline font-black text-sm tracking-[0.22em] text-white uppercase leading-none">
              RAQIM
            </span>
            <span className="font-mono text-[9px] font-bold tracking-widest px-1.5 py-0.5 rounded bg-cyan-500/10 text-cyan-400 border border-cyan-500/25 leading-none">
              CORE
            </span>
          </div>
          <span className="font-mono text-[9px] tracking-[0.16em] text-zinc-500 uppercase mt-1 leading-none">
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
