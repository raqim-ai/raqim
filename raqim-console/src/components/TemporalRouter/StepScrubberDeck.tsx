'use client';

import React, { useRef, useEffect } from 'react';
import { TimelineNode, formatTxIdHex } from '../../lib/api';
import {
  GitCommit,
  Terminal,
  Cpu,
  Database,
  ChevronsLeft,
  ChevronLeft,
  ChevronRight,
  ChevronsRight,
  Radio,
} from 'lucide-react';

interface StepScrubberDeckProps {
  timeline: TimelineNode[];
  selectedIndex: number;
  onSelectIndex: (index: number) => void;
  isLoading: boolean;
}

const getStatusBadge = (status: string, payload: string) => {
  const normalized = status.toUpperCase();
  if (normalized.includes('TOOL') || payload.includes('[TOOL')) {
    return {
      label: 'ToolExecution',
      style: 'bg-amber-500/10 border-amber-500/30 text-amber-400',
    };
  }
  if (normalized.includes('REASON') || payload.includes('[REASON')) {
    return {
      label: 'Reasoning',
      style: 'bg-purple-500/10 border-purple-500/30 text-purple-400',
    };
  }
  if (normalized.includes('IDLE') || normalized.includes('STANDBY')) {
    return {
      label: 'Idle',
      style: 'bg-zinc-800 border-zinc-700 text-zinc-400',
    };
  }
  return {
    label: status || 'Committed',
    style: 'bg-cyan-500/10 border-cyan-500/30 text-cyan-400',
  };
};

export function StepScrubberDeck({
  timeline,
  selectedIndex,
  onSelectIndex,
  isLoading,
}: StepScrubberDeckProps) {
  const scrollContainerRef = useRef<HTMLDivElement>(null);

  // Auto-scroll active card into view
  useEffect(() => {
    if (scrollContainerRef.current) {
      const activeEl = scrollContainerRef.current.querySelector('[data-active="true"]') as HTMLElement;
      if (activeEl) {
        activeEl.scrollIntoView({ behavior: 'smooth', block: 'nearest', inline: 'center' });
      }
    }
  }, [selectedIndex]);

  const handlePrev = () => {
    if (selectedIndex > 0) onSelectIndex(selectedIndex - 1);
  };

  const handleNext = () => {
    if (selectedIndex < timeline.length - 1) onSelectIndex(selectedIndex + 1);
  };

  const handleFirst = () => onSelectIndex(0);
  const handleLast = () => onSelectIndex(Math.max(timeline.length - 1, 0));

  return (
    <div className="bg-zinc-950/80 border border-zinc-800/80 rounded-sm overflow-hidden flex flex-col shrink-0 shadow-lg select-none">
      {/* Scrubber Controls Bar */}
      <div className="bg-zinc-900/90 border-b border-zinc-800 px-3 py-1.5 flex items-center justify-between gap-3 text-xs font-mono">
        <div className="flex items-center gap-2">
          <GitCommit className="w-3.5 h-3.5 text-cyan-400" />
          <span className="font-sans text-xs uppercase tracking-wider font-bold text-zinc-300">
            Causal Execution Timeline &amp; Step Scrubber
          </span>
          <span className="px-1.5 py-0.2 rounded-xs bg-zinc-800 border border-zinc-700 text-[10px] text-cyan-400 font-bold">
            {timeline.length} CAUSAL STEPS
          </span>
        </div>

        {/* Step Navigation Buttons */}
        <div className="flex items-center gap-1">
          <button
            onClick={handleFirst}
            disabled={selectedIndex <= 0 || timeline.length === 0}
            title="First Step"
            className="p-1 rounded-xs bg-zinc-950 border border-zinc-800 text-zinc-400 hover:text-white disabled:opacity-30 transition-colors"
          >
            <ChevronsLeft className="w-3.5 h-3.5" />
          </button>
          <button
            onClick={handlePrev}
            disabled={selectedIndex <= 0 || timeline.length === 0}
            title="Previous Step"
            className="p-1 rounded-xs bg-zinc-950 border border-zinc-800 text-zinc-400 hover:text-white disabled:opacity-30 transition-colors"
          >
            <ChevronLeft className="w-3.5 h-3.5" />
          </button>

          <span className="px-2 font-bold text-white text-[11px]">
            STEP #{timeline.length > 0 ? selectedIndex + 1 : 0} / {timeline.length}
          </span>

          <button
            onClick={handleNext}
            disabled={selectedIndex >= timeline.length - 1 || timeline.length === 0}
            title="Next Step"
            className="p-1 rounded-xs bg-zinc-950 border border-zinc-800 text-zinc-400 hover:text-white disabled:opacity-30 transition-colors"
          >
            <ChevronRight className="w-3.5 h-3.5" />
          </button>
          <button
            onClick={handleLast}
            disabled={selectedIndex >= timeline.length - 1 || timeline.length === 0}
            title="Head Step"
            className="p-1 rounded-xs bg-zinc-950 border border-zinc-800 text-zinc-400 hover:text-white disabled:opacity-30 transition-colors"
          >
            <ChevronsRight className="w-3.5 h-3.5" />
          </button>
        </div>
      </div>

      {/* Horizontal Scrubber Strip */}
      <div
        ref={scrollContainerRef}
        className="p-2.5 overflow-x-auto flex items-stretch gap-2.5 min-h-[105px] bg-zinc-950 scrollbar-thin scrollbar-thumb-zinc-800"
      >
        {isLoading ? (
          <div className="flex-1 flex items-center justify-center py-6 text-zinc-400 font-mono text-xs gap-2">
            <div className="w-4 h-4 border-2 border-emerald-400 border-t-transparent rounded-full animate-spin" />
            <span>FETCHING CAUSAL TIMELINE FROM LANCEDB + WAL...</span>
          </div>
        ) : timeline.length === 0 ? (
          <div className="flex-1 flex items-center justify-center py-6 text-zinc-500 font-mono text-xs uppercase">
            [ ZERO RECORDED STEPS FOR THIS ENCLAVE ]
          </div>
        ) : (
          timeline.map((node, index) => {
            const isSelected = index === selectedIndex;
            const statusBadge = getStatusBadge(node.agent_status, node.payload_preview);
            const previewText = node.payload_preview
              ? node.payload_preview.length > 60
                ? `${node.payload_preview.slice(0, 60)}...`
                : node.payload_preview
              : '[State Transition Committed]';

            return (
              <div
                key={`${node.tx_id}-${index}`}
                data-active={isSelected}
                onClick={() => onSelectIndex(index)}
                className={`w-64 shrink-0 p-2.5 rounded-sm border cursor-pointer transition-all duration-150 flex flex-col justify-between group ${
                  isSelected
                    ? 'bg-zinc-900 border-emerald-500 ring-2 ring-emerald-500/40 shadow-[0_0_15px_rgba(16,185,129,0.3)]'
                    : 'bg-zinc-900/60 border-zinc-800/90 hover:border-zinc-700 hover:bg-zinc-900/80'
                }`}
              >
                {/* Node Header: STEP #(index + 1) | timestamp */}
                <div className="flex items-center justify-between gap-1 mb-1 font-mono text-[10px]">
                  <span className={`font-bold ${isSelected ? 'text-emerald-400' : 'text-zinc-300'}`}>
                    STEP #{index + 1}
                  </span>
                  <span className="text-zinc-500 text-[9px] truncate max-w-[100px]" title={node.timestamp}>
                    {node.timestamp || '00:00:00'}
                  </span>
                </div>

                {/* Status Pill */}
                <div className="my-0.5">
                  <span
                    className={`inline-block px-1.5 py-0.2 rounded-xs border text-[9px] font-bold uppercase font-mono ${statusBadge.style}`}
                  >
                    {statusBadge.label}
                  </span>
                </div>

                {/* Payload Preview (slice 0, 60) */}
                <p className="text-zinc-300 text-[10px] font-mono leading-snug my-1 line-clamp-2">
                  {previewText}
                </p>
              </div>
            );
          })
        )}
      </div>
    </div>
  );
}
