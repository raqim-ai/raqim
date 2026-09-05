'use client';

import React, { useEffect } from 'react';
import { MainLayout } from '../Layout/MainLayout';
import { GlobalStatusBar } from './GlobalStatusBar';
import { MetricCardsGrid } from './MetricCardsGrid';
import { LiveSemanticFirehose } from './LiveSemanticFirehose';
import { HardwareVitalsPanel } from './HardwareVitalsPanel';
import { useSwarmStream } from '../../lib/hooks/useSwarmStream';
import { useSwarmStore, ClusterInfoData, VaultTelemetry, DashboardCardsData } from '../../lib/store/useSwarmStore';

interface CommandDeckClientProps {
  initialCards: DashboardCardsData | null;
  initialVaultTelemetry: VaultTelemetry | null;
  initialClusterInfo: ClusterInfoData | null;
  initialAliases: Record<string, string>;
}

export function CommandDeckClient({
  initialCards,
  initialVaultTelemetry,
  initialClusterInfo,
  initialAliases,
}: CommandDeckClientProps) {
  // Ingress listeners
  useSwarmStream();

  const setDashboardCards = useSwarmStore((state) => state.setDashboardCards);
  const setVaultTelemetry = useSwarmStore((state) => state.setVaultTelemetry);
  const setClusterInfo = useSwarmStore((state) => state.setClusterInfo);
  const setAgentAliases = useSwarmStore((state) => state.setAgentAliases);
  const fetchInitialTopology = useSwarmStore((state) => state.fetchInitialTopology);

  useEffect(() => {
    if (initialCards) setDashboardCards(initialCards);
    if (initialVaultTelemetry) setVaultTelemetry(initialVaultTelemetry);
    if (initialClusterInfo) setClusterInfo(initialClusterInfo);
    if (initialAliases) setAgentAliases(initialAliases);
    fetchInitialTopology();
  }, [
    initialCards,
    initialVaultTelemetry,
    initialClusterInfo,
    initialAliases,
    setDashboardCards,
    setVaultTelemetry,
    setClusterInfo,
    setAgentAliases,
    fetchInitialTopology,
  ]);

  return (
    <MainLayout title="Command Flight Deck // Sovereign Root">
      <div className="flex flex-col h-full w-full bg-[#050811] overflow-hidden">
        {/* 1. Global Status Ribbon & Top Navigation */}
        <GlobalStatusBar />

        {/* 2. Main Flight Deck Content Area */}
        <main className="flex-1 flex flex-col p-3 gap-3 min-h-0 overflow-hidden">
          {/* Tactical Stat Ribbon */}
          <MetricCardsGrid
            initialCards={initialCards}
            initialVaultTelemetry={initialVaultTelemetry}
          />

          {/* Lower Split Pane: Firehose Table (Left) + Hardware Telemetry (Right) */}
          <div className="flex-1 grid grid-cols-1 lg:grid-cols-12 gap-3 min-h-0 overflow-hidden">
            {/* Live Semantic Firehose Deck */}
            <div className="lg:col-span-7 xl:col-span-8 flex flex-col min-h-0 h-full overflow-hidden">
              <LiveSemanticFirehose />
            </div>

            {/* Hardware Vitals & Ingress Telemetry Panel (Right-Side 2x2 Grid) */}
            <div className="lg:col-span-5 xl:col-span-4 flex flex-col min-h-0 h-full overflow-hidden">
              <HardwareVitalsPanel />
            </div>
          </div>
        </main>
      </div>
    </MainLayout>
  );
}
