'use client';

import React, { useEffect, useState, useRef } from 'react';
import { Terminal, Trash2, Radio, ArrowDownCircle } from 'lucide-react';
import { getFirehoseStreamUrl } from '../../lib/api';
import { useSwarmStore } from '../../lib/store/useSwarmStore';

interface LiveTerminalEvent {
  id: string;
  timestamp: string;
  type: string;
  agent_hex: string;
  message: string;
}

interface PhantomTerminalProps {
  selectedAgentHex?: string;
}

export function PhantomTerminal({ selectedAgentHex }: PhantomTerminalProps) {
  const [logs, setLogs] = useState<LiveTerminalEvent[]>([]);
  const [isConnected, setIsConnected] = useState(false);
  const [autoScroll, setAutoScroll] = useState(true);
  const logsEndRef = useRef<HTMLDivElement>(null);

  // Ingest thoughts from store and/or direct firehose SSE
  const thoughts = useSwarmStore((state) => state.thoughts);
  const thoughtOrder = useSwarmStore((state) => state.thoughtOrder);

  // Reconcile initial historical logs from thoughts stream for the selected agent
  useEffect(() => {
    if (!selectedAgentHex) return;
    const initialList: LiveTerminalEvent[] = [];

    for (let i = 0; i < thoughtOrder.length; i++) {
      const t = thoughts[thoughtOrder[i]];
      if (t && (t.agent_hex === selectedAgentHex || selectedAgentHex === 'ALL')) {
        initialList.push({
          id: `thought-${t.tx_id}`,
          timestamp: t.timestamp ? new Date(t.timestamp).toLocaleTimeString() : new Date().toLocaleTimeString(),
          type: t.status || 'COMMITTED',
          agent_hex: t.agent_hex,
          message: t.text,
        });
      }
    }

    setLogs(initialList.slice(-150));
  }, [selectedAgentHex, thoughts, thoughtOrder]);

  // Connect to SSE stream at /v1/system/firehose
  useEffect(() => {
    let eventSource: EventSource | null = null;

    try {
      const streamUrl = getFirehoseStreamUrl();
      eventSource = new EventSource(streamUrl);

      eventSource.onopen = () => {
        setIsConnected(true);
      };

      eventSource.onmessage = (event) => {
        if (!event.data || !event.data.trim()) return;
        try {
          const rawData = JSON.parse(event.data);
          const eventType = rawData.event_type || rawData.type || 'ThoughtCommitted';
          const agent = rawData.agent_hex || rawData.source_hex || '';

          if (selectedAgentHex && agent && agent !== selectedAgentHex && selectedAgentHex !== 'ALL') {
            return;
          }

          const messageText =
            rawData.text ||
            rawData.payload ||
            rawData.question_payload ||
            rawData.message ||
            JSON.stringify(rawData);

          const newLog: LiveTerminalEvent = {
            id: Math.random().toString(36).substring(2, 9),
            timestamp: new Date().toLocaleTimeString(),
            type: eventType,
            agent_hex: agent,
            message: messageText,
          };

          setLogs((prev) => [...prev.slice(-200), newLog]);
        } catch {
          // Ignore keepalives
        }
      };

      eventSource.onerror = () => {
        setIsConnected(false);
      };
    } catch {
      setIsConnected(false);
    }

    return () => {
      if (eventSource) {
        eventSource.close();
      }
    };
  }, [selectedAgentHex]);

  // Auto-scroll effect
  useEffect(() => {
    if (autoScroll && logsEndRef.current) {
      logsEndRef.current.scrollIntoView({ behavior: 'smooth' });
    }
  }, [logs, autoScroll]);

  const handleClear = () => {
    setLogs([]);
  };

  const getLogBadge = (type: string) => {
    switch (type.toUpperCase()) {
      case 'AEGISALERT':
      case 'HALTED':
      case 'REJECTED':
        return 'text-rose-400 bg-rose-950/60 border-rose-800/80';
      case 'A2AMESSAGEROUTED':
        return 'text-cyan-300 bg-cyan-950/60 border-cyan-800/80';
      case 'TOOL_EXEC':
        return 'text-amber-300 bg-amber-950/60 border-amber-800/80';
      case 'REASONING':
        return 'text-purple-300 bg-purple-950/60 border-purple-800/80';
      default:
        return 'text-emerald-400 bg-emerald-950/60 border-emerald-800/80';
    }
  };

  return (
    <div className="flex-1 flex flex-col min-h-0 bg-zinc-950 border border-zinc-800/80 rounded-sm overflow-hidden shadow-lg font-mono text-xs select-none">
      {/* Header */}
      <div className="bg-zinc-900/90 border-b border-zinc-800 px-3 py-2 flex items-center justify-between gap-2 shrink-0 select-none">
        <div className="flex items-center gap-2">
          <Terminal className="w-3.5 h-3.5 text-emerald-400" />
          <span className="font-sans text-xs uppercase tracking-wider font-bold text-white">
            Live Stream Terminal ({selectedAgentHex ? `${selectedAgentHex.slice(0, 8)}...` : 'All Enclaves'})
          </span>
          <div className="flex items-center gap-1 text-[10px]">
            <Radio className={`w-3 h-3 ${isConnected ? 'text-emerald-400 animate-pulse' : 'text-zinc-500'}`} />
            <span className={isConnected ? 'text-emerald-400 font-bold' : 'text-zinc-500'}>
              {isConnected ? 'STREAMING' : 'STANDBY'}
            </span>
          </div>
        </div>

        <div className="flex items-center gap-2 text-[10px]">
          <button
            onClick={() => setAutoScroll(!autoScroll)}
            className={`flex items-center gap-1 px-2 py-0.5 rounded-xs border transition-colors ${
              autoScroll
                ? 'bg-zinc-900 border-zinc-700 text-zinc-200'
                : 'bg-zinc-950 border-zinc-800 text-zinc-500'
            }`}
          >
            <ArrowDownCircle className="w-3 h-3" />
            <span>AUTO-SCROLL: {autoScroll ? 'ON' : 'OFF'}</span>
          </button>

          <button
            onClick={handleClear}
            className="p-1 text-zinc-400 hover:text-rose-400 rounded-xs hover:bg-zinc-800 transition-colors"
            title="Clear Terminal Logs"
          >
            <Trash2 className="w-3.5 h-3.5" />
          </button>
        </div>
      </div>

      {/* Terminal Output */}
      <div className="flex-1 p-3 overflow-y-auto space-y-1.5 bg-black/90 text-xs leading-relaxed scrollbar-thin scrollbar-thumb-zinc-800">
        {logs.length === 0 ? (
          <div className="py-16 text-center text-zinc-600 font-mono text-xs uppercase">
            [ AWAITING LIVE THOUGHT LOGS FOR ENCLAVE {selectedAgentHex ? selectedAgentHex.slice(0, 8) : ''}... ]
          </div>
        ) : (
          logs.map((log) => (
            <div key={log.id} className="flex items-start gap-2">
              <span className="text-zinc-500 text-[10px] shrink-0 select-none">
                [{log.timestamp}]
              </span>
              <span
                className={`inline-block px-1.5 py-0.2 rounded-xs border text-[9px] font-bold uppercase shrink-0 ${getLogBadge(
                  log.type
                )}`}
              >
                {log.type}
              </span>
              <span className="break-all text-zinc-200 text-[11px] leading-snug">
                {log.message}
              </span>
            </div>
          ))
        )}
        <div ref={logsEndRef} />
      </div>
    </div>
  );
}
