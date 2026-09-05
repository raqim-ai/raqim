'use client';

import React, { useState, useEffect, useMemo } from 'react';
import { MainLayout } from '../Layout/MainLayout';
import { VaultTelemetryRibbon } from './VaultTelemetryRibbon';
import { UnifiedSearchWorkbench } from './UnifiedSearchWorkbench';
import { MerkleProofInspector } from './MerkleProofInspector';
import { VaultTelemetry, VaultSearchResult, ClusterShard } from '../../lib/api';
import { fetchVaultTelemetry, fetchVaultSearchResults, triggerCompactionAction } from '../../actions/vault';
import { fetchTopology, fetchDashboardCards } from '../../actions/admin';
import { useSwarmStore } from '../../lib/store/useSwarmStore';
import { useSwarmStream } from '../../lib/hooks/useSwarmStream';
import { useSearchParams, useRouter } from 'next/navigation';
import { CheckCircle, AlertCircle, Zap, Loader2 } from 'lucide-react';

interface VaultClientLayoutProps {
  initialTelemetry: VaultTelemetry | null;
  initialResults: VaultSearchResult[];
  initialTxId: string | null;
  initialTopology: ClusterShard[];
}

interface ToastMessage {
  id: string;
  type: 'success' | 'error';
  text: string;
}

export function VaultClientLayout({
  initialTelemetry,
  initialResults,
  initialTxId,
  initialTopology,
}: VaultClientLayoutProps) {
  useSwarmStream();

  const router = useRouter();
  const searchParams = useSearchParams();
  const queryTxId = searchParams.get('tx_id') || initialTxId;

  const [telemetry, setTelemetry] = useState<VaultTelemetry | null>(initialTelemetry);
  const [results, setResults] = useState<VaultSearchResult[]>(initialResults);
  const [selectedTxIdHex, setSelectedTxIdHex] = useState<string | null>(queryTxId);
  const [isLoadingSearch, setIsLoadingSearch] = useState(false);
  const [isCompacting, setIsCompacting] = useState(false);
  const [shards, setShards] = useState<ClusterShard[]>(initialTopology);
  const [toasts, setToasts] = useState<ToastMessage[]>([]);

  const setStoreTelemetry = useSwarmStore((state) => state.setVaultTelemetry);
  const setStoreCards = useSwarmStore((state) => state.setDashboardCards);

  const showToast = (text: string, type: 'success' | 'error' = 'success') => {
    const id = Math.random().toString(36).substring(2, 9);
    setToasts((prev) => [...prev, { id, type, text }]);
    setTimeout(() => {
      setToasts((prev) => prev.filter((t) => t.id !== id));
    }, 4000);
  };

  useEffect(() => {
    if (initialTelemetry) setStoreTelemetry(initialTelemetry);
  }, [initialTelemetry, setStoreTelemetry]);

  // Sync telemetry every 5 seconds
  useEffect(() => {
    const syncData = async () => {
      try {
        const [tel, top] = await Promise.all([
          fetchVaultTelemetry(),
          fetchTopology(),
        ]);
        if (tel) {
          setTelemetry(tel);
          setStoreTelemetry(tel);
        }
        if (top) {
          setShards(top);
        }
      } catch (_e) {
        // Quiet poll error
      }
    };

    const interval = setInterval(syncData, 5000);
    return () => clearInterval(interval);
  }, [setStoreTelemetry]);

  // Extract distinct namespaces from shards
  const namespaces = useMemo(() => {
    const list = shards.map((s) => s.namespace);
    return Array.from(new Set(list));
  }, [shards]);

  const handleSearch = async (query: string, namespace: string, includeWal: boolean) => {
    setIsLoadingSearch(true);
    try {
      const searchRes = await fetchVaultSearchResults(query, namespace, includeWal);
      setResults(searchRes || []);
    } catch (_err) {
      setResults([]);
    } finally {
      setIsLoadingSearch(false);
    }
  };

  const handleSelectTxId = (txIdHex: string) => {
    setSelectedTxIdHex(txIdHex);
    // Update URL query parameter without full page reload
    router.replace(`/vault?tx_id=${encodeURIComponent(txIdHex)}`, { scroll: false });
  };

  const handleTriggerCompaction = async () => {
    if (isCompacting) return;
    setIsCompacting(true);
    try {
      const res = await triggerCompactionAction();
      if (res.success) {
        // On HTTP 200/202 response, trigger exact toast notification
        showToast(
          'WAL segment rotated. 2PC LanceDB assimilation initiated in background.',
          'success'
        );

        // Wait 1.5 seconds post-trigger and automatically re-fetch telemetry & dashboard cards
        setTimeout(async () => {
          try {
            const [updatedTel, updatedCards] = await Promise.all([
              fetchVaultTelemetry(),
              fetchDashboardCards(),
            ]);

            if (updatedTel) {
              setTelemetry(updatedTel);
              setStoreTelemetry(updatedTel);
            }
            if (updatedCards) {
              setStoreCards(updatedCards);
            }
          } catch {
            // Ignore background error
          }
        }, 1500);

        // Re-sync again after 3.5 seconds to capture finished 2PC LanceDB assimilation
        setTimeout(async () => {
          try {
            const [updatedTel, updatedCards] = await Promise.all([
              fetchVaultTelemetry(),
              fetchDashboardCards(),
            ]);

            if (updatedTel) {
              setTelemetry(updatedTel);
              setStoreTelemetry(updatedTel);
            }
            if (updatedCards) {
              setStoreCards(updatedCards);
            }
          } catch {
            // Ignore
          }
        }, 3500);
      } else {
        showToast(`COMPACTION FAILED: ${res.error || 'Unknown error'}`, 'error');
      }
    } catch (err: any) {
      showToast(`COMPACTION FAILED: ${err.message || 'Daemon unreachable'}`, 'error');
    } finally {
      setIsCompacting(false);
    }
  };

  // Compaction Button styled per specification:
  // Industrial Zinc-900 border with subtle Amber/Cyan glow on hover
  // (border-cyan-500/30 text-cyan-400 bg-cyan-950/20 hover:bg-cyan-900/40)
  const compactionHeaderAction = (
    <button
      onClick={handleTriggerCompaction}
      disabled={isCompacting}
      title="Rotate active WAL segment and initiate 2PC assimilation into LanceDB"
      className="border border-cyan-500/30 text-cyan-400 bg-cyan-950/20 hover:bg-cyan-900/40 font-mono text-xs font-bold uppercase transition-all px-3.5 py-1.5 rounded-sm flex items-center gap-2 shadow-[0_0_12px_rgba(0,243,255,0.15)] hover:shadow-[0_0_20px_rgba(0,243,255,0.3)] disabled:opacity-50 disabled:cursor-not-allowed cursor-pointer"
    >
      {isCompacting ? (
        <>
          <Loader2 className="w-3.5 h-3.5 text-cyan-400 animate-spin" />
          <span>[⏳ ROTATING &amp; COMPACTING...]</span>
        </>
      ) : (
        <>
          <Zap className="w-3.5 h-3.5 text-cyan-400" />
          <span>[⚡ TRIGGER WAL COMPACTION]</span>
        </>
      )}
    </button>
  );

  return (
    <MainLayout
      title="Forensic Audit Vault // Cryptographic Verifier"
      headerAction={compactionHeaderAction}
    >
      <div className="flex flex-col h-full w-full bg-zinc-950 overflow-hidden p-3 gap-3">
        {/* 1. Vault Telemetry Ribbon with Compaction Trigger */}
        <VaultTelemetryRibbon
          telemetry={telemetry}
          onTriggerCompaction={handleTriggerCompaction}
          isCompacting={isCompacting}
        />

        {/* 2. 2-Column Tactical Workspace */}
        <div className="flex-1 grid grid-cols-1 lg:grid-cols-12 gap-3 min-h-0 overflow-hidden">
          {/* Left Column: Unified Search Workbench (55% width) */}
          <div className="lg:col-span-6 xl:col-span-7 flex flex-col min-h-0 h-full overflow-hidden">
            <UnifiedSearchWorkbench
              results={results}
              onSearch={handleSearch}
              onSelectTxId={handleSelectTxId}
              selectedTxIdHex={selectedTxIdHex}
              isLoading={isLoadingSearch}
              namespaces={namespaces}
            />
          </div>

          {/* Right Column: Merkle Proof Inspector (45% width) */}
          <div className="lg:col-span-6 xl:col-span-5 flex flex-col min-h-0 h-full overflow-hidden">
            <MerkleProofInspector
              initialTxIdHex={selectedTxIdHex}
              onTxIdChange={handleSelectTxId}
            />
          </div>
        </div>

        {/* 3. Floating Feedback Toasts */}
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
