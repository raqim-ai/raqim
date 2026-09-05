'use client';

import React, { useState, useEffect, useCallback } from 'react';
import { MainLayout } from '../Layout/MainLayout';
import { TemporalHeaderRibbon } from './TemporalHeaderRibbon';
import { StepScrubberDeck } from './StepScrubberDeck';
import { EffectDiffInspector } from './EffectDiffInspector';
import { PhantomTerminal } from './PhantomTerminal';
import { TimelineNode, ClusterEnclave, formatTxIdHex } from '../../lib/api';
import { fetchAgentTimeline, fetchClusterEnclaves } from '../../actions/admin';
import { useSwarmStore } from '../../lib/store/useSwarmStore';
import { useSwarmStream } from '../../lib/hooks/useSwarmStream';
import { CheckCircle, AlertCircle } from 'lucide-react';

interface TemporalClientLayoutProps {
  initialAgentHex: string;
  initialTimeline: TimelineNode[];
  initialAliases: Record<string, string>;
  initialEnclaves: ClusterEnclave[];
}

interface ToastMessage {
  id: string;
  type: 'success' | 'error';
  text: string;
}

export function TemporalClientLayout({
  initialAgentHex,
  initialTimeline,
  initialAliases,
  initialEnclaves = [],
}: TemporalClientLayoutProps) {
  useSwarmStream();

  const [selectedAgentHex, setSelectedAgentHex] = useState<string>(initialAgentHex);
  const [timeline, setTimeline] = useState<TimelineNode[]>(initialTimeline);
  const [selectedIndex, setSelectedIndex] = useState<number>(
    initialTimeline.length > 0 ? initialTimeline.length - 1 : 0
  );
  const [mode, setMode] = useState<'RECORD' | 'REPLAY'>('RECORD');
  const [isLoadingTimeline, setIsLoadingTimeline] = useState<boolean>(false);
  const [enclaves, setEnclaves] = useState<ClusterEnclave[]>(initialEnclaves);
  const [toasts, setToasts] = useState<ToastMessage[]>([]);

  const setStoreAliases = useSwarmStore((state) => state.setAgentAliases);
  const agentAliases = useSwarmStore((state) => state.agentAliases);

  useEffect(() => {
    if (initialAliases) setStoreAliases(initialAliases);
  }, [initialAliases, setStoreAliases]);

  // Sync cluster enclaves
  useEffect(() => {
    fetchClusterEnclaves().then((enc) => {
      if (enc && enc.length > 0) {
        setEnclaves(enc);
        if (!selectedAgentHex && enc[0]) {
          setSelectedAgentHex(enc[0].identity_hex);
        }
      }
    });
  }, [selectedAgentHex]);

  const showToast = (text: string, type: 'success' | 'error' = 'success') => {
    const id = Math.random().toString(36).substring(2, 9);
    setToasts((prev) => [...prev, { id, type, text }]);
    setTimeout(() => {
      setToasts((prev) => prev.filter((t) => t.id !== id));
    }, 4000);
  };

  // Load timeline for selected agent
  const loadTimeline = useCallback(async (agentHex: string) => {
    if (!agentHex) return;
    setIsLoadingTimeline(true);
    try {
      const nodes = await fetchAgentTimeline(agentHex);
      setTimeline(nodes || []);
      if (nodes && nodes.length > 0) {
        setSelectedIndex(nodes.length - 1);
        setMode('RECORD');
      } else {
        setSelectedIndex(0);
      }
    } catch {
      setTimeline([]);
    } finally {
      setIsLoadingTimeline(false);
    }
  }, []);

  useEffect(() => {
    loadTimeline(selectedAgentHex);
  }, [selectedAgentHex, loadTimeline]);

  const currentNode = timeline[selectedIndex] || null;
  const activeTxIdHex = currentNode ? formatTxIdHex(currentNode.tx_id) : null;

  return (
    <MainLayout title="Time Travel // Causal Execution Observatory">
      <div className="flex flex-col h-full w-full bg-zinc-950 overflow-hidden p-3 gap-3">
        {/* 1. Header Ribbon Controls */}
        <TemporalHeaderRibbon
          enclaves={enclaves}
          agentAliases={agentAliases}
          selectedAgentHex={selectedAgentHex}
          onSelectAgent={(hex) => {
            setSelectedAgentHex(hex);
            setMode('RECORD');
          }}
          mode={mode}
          activeTxIdHex={activeTxIdHex}
        />

        {/* 2. Full-Width Sequential Causal Scrubber */}
        <StepScrubberDeck
          timeline={timeline}
          selectedIndex={selectedIndex}
          onSelectIndex={(idx) => {
            setSelectedIndex(idx);
            if (idx < timeline.length - 1) {
              setMode('REPLAY');
            } else {
              setMode('RECORD');
            }
          }}
          isLoading={isLoadingTimeline}
        />

        {/* 3. Lower Workspace: 2-Column Comparative Split + Right Stream Terminal */}
        <div className="flex-1 grid grid-cols-1 lg:grid-cols-12 gap-3 min-h-0 overflow-hidden">
          {/* Left: Side-Effect Boundary & Diff Inspector (60% width) */}
          <div className="lg:col-span-7 xl:col-span-7 flex flex-col min-h-0 h-full overflow-hidden">
            <EffectDiffInspector
              currentNode={currentNode}
              stepIndex={selectedIndex}
              agentHex={selectedAgentHex}
            />
          </div>

          {/* Right: Live Firehose Stream Terminal (40% width) */}
          <div className="lg:col-span-5 xl:col-span-5 flex flex-col min-h-0 h-full overflow-hidden">
            <PhantomTerminal selectedAgentHex={selectedAgentHex} />
          </div>
        </div>

        {/* 4. Floating Feedback Toasts */}
        <div className="fixed bottom-4 left-4 z-50 flex flex-col gap-2 font-mono text-xs select-none">
          {toasts.map((toast) => (
            <div
              key={toast.id}
              className={`flex items-center gap-2 px-3.5 py-2.5 rounded-xs border shadow-2xl animate-in slide-in-from-bottom-2 duration-150 ${
                toast.type === 'success'
                  ? 'bg-emerald-950/90 border-emerald-500/80 text-emerald-300 shadow-[0_0_15px_rgba(16,185,129,0.2)]'
                  : 'bg-rose-950/90 border-rose-500/80 text-rose-300 shadow-[0_0_15px_rgba(244,63,94,0.2)]'
              }`}
            >
              {toast.type === 'success' ? (
                <CheckCircle className="w-4 h-4 text-emerald-400" />
              ) : (
                <AlertCircle className="w-4 h-4 text-rose-400" />
              )}
              <span>{toast.text}</span>
            </div>
          ))}
        </div>
      </div>
    </MainLayout>
  );
}
