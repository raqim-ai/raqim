import { fetchVaultTelemetry } from '../../actions/vault';
import { fetchTopology } from '../../actions/admin';
import { VaultClientLayout } from '../../components/Vault/VaultClientLayout';

interface AuditVaultPageProps {
  searchParams: Promise<{ tx_id?: string }>;
}

export default async function AuditVaultPage({ searchParams }: AuditVaultPageProps) {
  const resolvedParams = await searchParams;
  const initialTxId = resolvedParams?.tx_id || null;

  const [telemetry, topology] = await Promise.all([
    fetchVaultTelemetry().catch(() => null),
    fetchTopology().catch(() => []),
  ]);

  return (
    <VaultClientLayout
      initialTelemetry={telemetry}
      initialResults={[]}
      initialTxId={initialTxId}
      initialTopology={topology}
    />
  );
}
