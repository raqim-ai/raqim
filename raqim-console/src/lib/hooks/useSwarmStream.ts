'use client';

import { useEffect, useRef } from 'react';
import { useSwarmStore, UiThought, UiEvent, formatTxIdHex } from '../store/useSwarmStore';
import { fetchEventSource } from '@microsoft/fetch-event-source';
import { getFirehoseStreamUrl } from '../api';

const inferStatus = (text: string, path: string): UiThought['status'] => {
  const lower = (text + ' ' + path).toLowerCase();
  if (lower.includes('halt') || lower.includes('error') || lower.includes('quarantine') || lower.includes('drop')) {
    return 'HALTED';
  }
  if (lower.includes('tool') || lower.includes('exec') || lower.includes('action') || lower.includes('call')) {
    return 'TOOL_EXEC';
  }
  if (lower.includes('reason') || lower.includes('think') || lower.includes('eval') || lower.includes('query')) {
    return 'REASONING';
  }
  return 'COMMITTED';
};

export function useSwarmStream() {
  const batchAddThoughts = useSwarmStore((state) => state.batchAddThoughts);
  const processUiEvents = useSwarmStore((state) => state.processUiEvents);
  const setDaemonOnline = useSwarmStore((state) => state.setDaemonOnline);
  const pruneEphemeralEdges = useSwarmStore((state) => state.pruneEphemeralEdges);
  const tickRollingMetrics = useSwarmStore((state) => state.tickRollingMetrics);
  const isPaused = useSwarmStore((state) => state.isPaused);

  const isPausedRef = useRef(isPaused);
  isPausedRef.current = isPaused;

  const thoughtsBufferRef = useRef<UiThought[]>([]);
  const eventsBufferRef = useRef<UiEvent[]>([]);
  const rAF_Ref = useRef<number>(0);

  useEffect(() => {
    // 1. Setup requestAnimationFrame batching for smooth 60fps rendering
    const flushBuffer = () => {
      if (!isPausedRef.current) {
        if (thoughtsBufferRef.current.length > 0) {
          batchAddThoughts([...thoughtsBufferRef.current]);
          thoughtsBufferRef.current = [];
        }
        if (eventsBufferRef.current.length > 0) {
          processUiEvents([...eventsBufferRef.current]);
          eventsBufferRef.current = [];
        }
      }
      rAF_Ref.current = requestAnimationFrame(flushBuffer);
    };

    rAF_Ref.current = requestAnimationFrame(flushBuffer);

    // 2. Setup lifecycle-managed intervals
    const metricsInterval = setInterval(() => {
      tickRollingMetrics();
    }, 1000);

    const pruneInterval = setInterval(() => {
      pruneEphemeralEdges();
    }, 100);

    // 3. Connect to canonical SSE Firehose endpoint
    const controller = new AbortController();
    const sseUrl = getFirehoseStreamUrl();

    fetchEventSource(sseUrl, {
      method: 'GET',
      headers: {
        'Accept': 'text/event-stream',
      },
      signal: controller.signal,
      async onopen(res) {
        if (res.ok && res.headers.get('content-type')?.includes('text/event-stream')) {
          setDaemonOnline(true, null);
          return;
        } else {
          setDaemonOnline(false, `DAEMON_HTTP_${res.status}`);
        }
      },
      onmessage(event) {
        // Ignore non-string or empty / whitespace data
        if (!event.data || typeof event.data !== 'string') return;
        const trimmed = event.data.trim();
        // Ignore keep-alive heartbeats, pings, or comments
        if (!trimmed || trimmed.startsWith(':') || trimmed === 'ping' || trimmed === 'keepalive' || trimmed === '""') {
          return;
        }

        try {
          const rawData = JSON.parse(trimmed);
          const eventType = rawData.event_type || rawData.type;
          const now = Date.now();

          if (eventType === 'ThoughtCommited' || eventType === 'ThoughtCommitted') {
            const rawTx = rawData.tx_id;
            const numericTx = typeof rawTx === 'number' ? rawTx : (parseInt(rawTx, 16) || now);
            const hexTx = typeof rawTx === 'string' && rawTx.length >= 10 ? rawTx : formatTxIdHex(rawTx);

            const text = rawData.text || rawData.payload || '';
            const path = rawData.intent_path || rawData.namespace || '/rqm_core';

            const data: UiThought = {
              tx_id: numericTx,
              tx_id_hex: hexTx,
              agent_hex: rawData.agent_hex || '0xUNKNOWN',
              intent_path: path,
              text,
              status: inferStatus(text, path),
              is_a2a_query: path?.includes('/a2a/') || false,
              parent_tx_id: numericTx > 0 ? numericTx - 1 : null,
              timestamp: rawData.timestamp || now,
            };
            thoughtsBufferRef.current.push(data);

            eventsBufferRef.current.push({
              event_type: 'ThoughtCommitted',
              agent_hex: data.agent_hex,
              intent_path: data.intent_path,
              tx_id: data.tx_id,
              tx_id_hex: data.tx_id_hex,
              text: data.text,
            });
          } else if (eventType === 'A2aMessageRouted') {
            eventsBufferRef.current.push({
              event_type: 'A2aMessageRouted',
              source_hex: rawData.source_hex,
              target_hex: rawData.target_hex,
              namespace: rawData.namespace,
              question_payload: rawData.question_payload || '',
              answer_payload: rawData.answer_payload || '',
              latency_ms: rawData.latency_ms || 0,
            });
          } else if (eventType === 'AegisAlert') {
            eventsBufferRef.current.push({
              event_type: 'AegisAlert',
              record: {
                agent_hex: rawData.record?.agent_hex || rawData.agent_hex || 'UNKNOWN',
                violation_type: rawData.record?.violation_type || rawData.violation_type || 'SECURITY_VIOLATION',
                attempted_path: rawData.record?.attempted_path || rawData.attempted_path || 'UNKNOWN',
                payload_preview: rawData.record?.payload_preview || rawData.payload_preview || '',
                timestamp: rawData.record?.timestamp || Date.now(),
              },
            });
          } else if (eventType === 'RealityForked') {
            eventsBufferRef.current.push({
              event_type: 'RealityForked',
              agent_id: rawData.agent_id,
              original_namespace: rawData.original_namespace,
              phantom_namespace: rawData.phantom_namespace,
              step_ordinal: rawData.step_ordinal,
              tx_id: rawData.tx_id,
            });
          }
          setDaemonOnline(true, null);
        } catch {
          // Quietly ignore malformed non-JSON keepalive frames
        }
      },
      onclose() {
        setDaemonOnline(false, 'DAEMON_STREAM_CLOSED');
      },
      onerror() {
        setDaemonOnline(false, 'DAEMON_UNREACHABLE');
      },
    }).catch(() => {
      setDaemonOnline(false, 'DAEMON_UNREACHABLE');
    });

    return () => {
      cancelAnimationFrame(rAF_Ref.current);
      clearInterval(metricsInterval);
      clearInterval(pruneInterval);
      controller.abort();
    };
  }, [
    batchAddThoughts,
    processUiEvents,
    setDaemonOnline,
    pruneEphemeralEdges,
    tickRollingMetrics,
  ]);
}
