import { fetchAgentAliases } from '../../actions/aliases';
import { fetchClusterEnclaves, fetchAgentTimeline } from '../../actions/admin';
import { TemporalClientLayout } from '../../components/TemporalRouter/TemporalClientLayout';

export default async function RouterPage() {
  const [agentAliases, enclaves] = await Promise.all([
    fetchAgentAliases().catch(() => ({})),
    fetchClusterEnclaves().catch(() => []),
  ]);

  const defaultAgentHex =
    enclaves.length > 0
      ? enclaves[0].identity_hex
      : Object.keys(agentAliases)[0] || '096da8e8a1b2c3d4e5f60718293a4b5c';

  const initialTimeline = await fetchAgentTimeline(defaultAgentHex).catch(() => []);

  return (
    <TemporalClientLayout
      initialAgentHex={defaultAgentHex}
      initialTimeline={initialTimeline}
      initialAliases={agentAliases}
      initialEnclaves={enclaves}
    />
  );
}
