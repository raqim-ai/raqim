'use client';

import React from 'react';
import { Sidebar } from './Sidebar';
import { usePathname } from 'next/navigation';
import { motion, AnimatePresence } from 'framer-motion';
import { useSwarmStore } from '../../lib/store/useSwarmStore';
import { RAQIM_DAEMON_BASE_URL } from '../../lib/api';

export function MainLayout({
  children,
  title,
  headerAction,
}: {
  children: React.ReactNode;
  title: string;
  headerAction?: React.ReactNode;
}) {
  const pathname = usePathname();
  const isTopology = pathname === '/topology';
  const isRouter = pathname === '/router';
  const isNoHeader = isTopology || isRouter;

  const daemonOnline = useSwarmStore((state) => state.daemonOnline);
  const currentVitals = useSwarmStore((state) => state.currentVitals);

  const ramMb = Math.round(
    currentVitals?.process_memory_mb ??
    currentVitals?.wasm_memory_mb ??
    currentVitals?.process_rss_mb ??
    0
  );

  return (
    <div className="bg-surface text-on-surface antialiased h-screen w-screen overflow-hidden flex flex-col selection:bg-primary-container/30">
      {/* ── Disconnected Banner ── */}
      {!daemonOnline && (
        <div className="bg-[#ff003c]/15 border-b border-[#ff003c]/40 px-6 py-1.5 flex items-center justify-between z-50 shrink-0">
          <div className="flex items-center gap-3">
            <span className="w-2 h-2 rounded-full bg-[#ff003c] animate-pulse"></span>
            <span className="font-mono text-xs text-[#ff003c] font-bold tracking-wider">
              [DAEMON DISCONNECTED: {RAQIM_DAEMON_BASE_URL.replace('http://', '')}]
            </span>
          </div>
          <span className="font-mono text-[10px] text-zinc-400 uppercase tracking-widest">
            Awaiting raqim-core daemon heartbeat...
          </span>
        </div>
      )}

      {/* ── Body row: Sidebar + Main Column ── */}
      <div className="flex flex-1 overflow-hidden min-h-0">
        <Sidebar />

        <main
          className={`flex-1 flex flex-col ${
            isNoHeader ? 'bg-surface' : 'bg-surface-container-low'
          } overflow-hidden relative min-h-0`}
        >
          {!isNoHeader && (
            <header className="flex justify-between items-center w-full px-8 py-6 bg-surface z-30 shrink-0 border-b border-outline-variant/10">
              <div className="flex items-center gap-4">
                <h1 className="font-headline text-3xl font-black tracking-tight text-on-surface uppercase">
                  {title}
                </h1>
                <div className="bg-surface-container-high px-3 py-1 rounded-sm outline outline-1 outline-outline-variant/15 outline-offset-[-1px] flex items-center gap-2">
                  <span
                    className={`w-2 h-2 rounded-full ${
                      daemonOnline ? 'bg-secondary animate-pulse' : 'bg-[#ff003c]'
                    }`}
                  ></span>
                  <span className="font-mono text-[10px] text-secondary uppercase tracking-widest">
                    {daemonOnline ? 'Live Swarm Active' : 'Offline / Standby'}
                  </span>
                </div>
              </div>

              <div className="flex items-center gap-4">
                {headerAction}
                {pathname === '/firewall' && (
                  <div className="text-[#ef4444] border border-[#ef4444]/30 bg-[#ef4444]/10 px-3.5 py-1.5 rounded-sm font-mono text-[10px] uppercase tracking-[0.2em] font-bold shadow-[0_0_10px_rgba(239,68,68,0.15)]">
                    [ AEGIS ENFORCEMENT: STRICT ]
                  </div>
                )}
              </div>
            </header>
          )}

          {/* Scrollable page content */}
          <div className="flex-1 min-h-0 overflow-hidden relative">
            <AnimatePresence mode="wait">
              <motion.div
                key={pathname}
                initial={{ opacity: 0, y: 10 }}
                animate={{ opacity: 1, y: 0 }}
                exit={{ opacity: 0, y: -10 }}
                transition={{ type: 'spring', stiffness: 350, damping: 28 }}
                style={{ height: '100%', width: '100%' }}
              >
                {children}
              </motion.div>
            </AnimatePresence>
          </div>

          {/* ── Footer ── */}
          {!isRouter && (
            <footer className="shrink-0 border-t border-zinc-800 bg-zinc-950 z-30 relative">
              <div className="flex items-center justify-between px-8 py-3">
                {/* Left: OS Identity */}
                <div className="flex items-center gap-5">
                  <div className="flex items-center gap-2">
                    <span
                      className={`w-1.5 h-1.5 rounded-full ${
                        daemonOnline ? 'bg-secondary animate-pulse' : 'bg-[#ff003c]'
                      } shrink-0`}
                    ></span>
                    <span className="font-mono text-[10px] font-bold uppercase tracking-[0.18em] text-zinc-300">
                      RAQIM CONSOLE
                    </span>
                    <span className="font-mono text-[10px] text-zinc-600">
                      v1.0.0 (Local Engine)
                    </span>
                  </div>
                  <div className="h-3 w-px bg-zinc-800 shrink-0"></div>
                  <span className="font-mono text-[10px] uppercase tracking-[0.15em] text-secondary">
                    {currentVitals
                      ? `CPU: ${currentVitals.cpu_load_percent.toFixed(1)}% | RAM: ${ramMb}MB`
                      : 'STANDALONE MODE'}
                  </span>
                </div>

                {/* Center: Node status */}
                <div className="flex items-center gap-2.5">
                  <span className="font-mono text-[9px] uppercase tracking-widest text-zinc-600">
                    DAEMON STATUS
                  </span>
                  <span
                    className={`font-mono text-[10px] uppercase tracking-wider ${
                      daemonOnline ? 'text-[#00f3ff]' : 'text-[#ff003c]'
                    } flex items-center gap-1.5`}
                  >
                    <span
                      className={`w-1.5 h-1.5 rounded-full ${
                        daemonOnline
                          ? 'bg-[#00f3ff] shadow-[0_0_6px_rgba(0,243,255,0.8)]'
                          : 'bg-[#ff003c]'
                      }`}
                    ></span>
                    {daemonOnline ? 'OPERATIONAL' : 'DISCONNECTED'}
                  </span>
                </div>

                {/* Right: Endpoint */}
                <div className="flex items-center gap-3">
                  <span className="font-mono text-[9px] uppercase tracking-widest text-zinc-600">
                    BACKEND
                  </span>
                  <span className="font-mono text-[10px] text-primary-fixed-dim bg-primary-container/10 px-2.5 py-1 border border-primary-container/25 uppercase tracking-widest">
                    {RAQIM_DAEMON_BASE_URL}
                  </span>
                </div>
              </div>
            </footer>
          )}
        </main>
      </div>
    </div>
  );
}
