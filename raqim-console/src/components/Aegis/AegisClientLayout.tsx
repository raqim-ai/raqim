'use client';

import React, { useState, useEffect, useMemo } from 'react';
import { MainLayout } from '../Layout/MainLayout';
import { AegisMetricsRibbon } from './AegisMetricsRibbon';
import { QuarantineTable } from './QuarantineTable';
import { RemediationDrawer } from './RemediationDrawer';
import { TokenBucketGauges } from './TokenBucketGauges';
import { CaMintStation } from './CaMintStation';
import { AegisMetricsData, QuarantineRecord } from '../../lib/api';
import { fetchAegisMetrics, fetchQuarantineList } from '../../actions/firewall';
import { useSwarmStore } from '../../lib/store/useSwarmStore';
import { useSwarmStream } from '../../lib/hooks/useSwarmStream';
import { CheckCircle, AlertCircle, Shield } from 'lucide-react';

interface AegisClientLayoutProps {
  initialMetrics: AegisMetricsData | null;
  initialQuarantineList: QuarantineRecord[];
}

interface ToastMessage {
  id: string;
  type: 'success' | 'error';
  text: string;
}

export function AegisClientLayout({
  initialMetrics,
  initialQuarantineList,
}: AegisClientLayoutProps) {
  useSwarmStream();
  
  const [metrics, setMetrics] = useState<AegisMetricsData | null>(initialMetrics);
  const [quarantineList, setQuarantineList] = useState<QuarantineRecord[]>(initialQuarantineList);
  const [selectedAgent, setSelectedAgent] = useState<QuarantineRecord | null>(null);
  const [isLoading, setIsLoading] = useState(false);
  const [toasts, setToasts] = useState<ToastMessage[]>([]);

  const setQuarantinedAgents = useSwarmStore((state) => state.setQuarantinedAgents);
  const liftStoreQuarantine = useSwarmStore((state) => state.liftQuarantine);

  const showToast = (text: string, type: 'success' | 'error' = 'success') => {
    const id = Math.random().toString(36).substring(2, 9);
    setToasts((prev) => [...prev, { id, type, text }]);
    setTimeout(() => {
      setToasts((prev) => prev.filter((t) => t.id !== id));
    }, 4000);
  };

  const syncAegisData = async () => {
    setIsLoading(true);
    try {
      const [m, q] = await Promise.all([
        fetchAegisMetrics(),
        fetchQuarantineList(),
      ]);
      if (m) setMetrics(m);
      setQuarantineList(q);
      setQuarantinedAgents(q.map((item) => item.agent_hex));
    } catch (_e) {
      // Quiet poll error
    } finally {
      setIsLoading(false);
    }
  };

  // Poll Aegis state every 5 seconds
  useEffect(() => {
    const interval = setInterval(syncAegisData, 5000);
    return () => clearInterval(interval);
  }, []);

  const handleRemediationSuccess = (agentHex: string) => {
    // Optimistic UI update
    setQuarantineList((prev) => prev.filter((r) => r.agent_hex !== agentHex));
    liftStoreQuarantine(agentHex);
    setMetrics((prev) =>
      prev
        ? { ...prev, total_quarantined: Math.max(prev.total_quarantined - 1, 0) }
        : null
    );
    showToast(`ENCLAVE ${agentHex.slice(0, 10)}... RESTORED TO MAIN TIMELINE`, 'success');
  };

  const handleRemediationError = (msg: string) => {
    showToast(`EVICTION FAILED: ${msg}`, 'error');
  };

  // Extract available security policy group names
  const availableGroups = useMemo(() => {
    if (!metrics?.active_policies || metrics.active_policies.length === 0) {
      return ['admin_group', 'finance_worker', 'default_agent'];
    }
    return metrics.active_policies.map((p) => p.group_name);
  }, [metrics]);

  return (
    <MainLayout title="Aegis Cryptographic Governance // Sovereign SOC">
      <div className="flex flex-col h-full w-full bg-[#080C14] overflow-hidden p-3 gap-3">
        {/* 1. Tactical Threat Metrics Ribbon */}
        <AegisMetricsRibbon
          metrics={metrics}
          activeQuarantinedCount={quarantineList.length}
        />

        {/* 2. 2-Column Tactical SOC Workspace */}
        <div className="flex-1 grid grid-cols-1 lg:grid-cols-12 gap-3 min-h-0 overflow-hidden">
          {/* Left Column: Active Quarantine Matrix (60% width) */}
          <div className="lg:col-span-7 xl:col-span-8 flex flex-col min-h-0 h-full overflow-hidden">
            <QuarantineTable
              quarantineList={quarantineList}
              onSelectAgent={(record) => setSelectedAgent(record)}
              onRefresh={syncAegisData}
              isLoading={isLoading}
            />
          </div>

          {/* Right Column: Token Gauges + CA Mint Station (40% width) */}
          <div className="lg:col-span-5 xl:col-span-4 flex flex-col gap-3 min-h-0 h-full overflow-y-auto pr-0.5">
            <TokenBucketGauges policies={metrics?.active_policies || []} />
            <CaMintStation availableGroups={availableGroups} />
          </div>
        </div>

        {/* 3. Anti-Relapse Remediation Drawer */}
        {selectedAgent && (
          <RemediationDrawer
            record={selectedAgent}
            onClose={() => setSelectedAgent(null)}
            onSuccess={handleRemediationSuccess}
            onError={handleRemediationError}
          />
        )}

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
