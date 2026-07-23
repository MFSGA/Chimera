import type { AgentNetworkSnapshot } from '@chimera/interface';
import {
  CableRounded,
  DnsRounded,
  FolderRounded,
  LanRounded,
  NetworkCheckRounded,
  SecurityRounded,
} from '@mui/icons-material';
import * as m from '@/paraglide/messages';
import {
  presentBoolean,
  presentConnectorState,
  presentCoreState,
  presentRate,
  presentRoutingMode,
  presentServiceState,
} from '../model/presenter';
import { AgentStatusCard, type AgentStatusRow } from './status-card';

const coreRows = (snapshot: AgentNetworkSnapshot): AgentStatusRow[] => [
  { label: m.agent_core_state(), value: presentCoreState(snapshot.core.state) },
  {
    label: `${m.agent_routing_mode()} · ${m.agent_desired()}`,
    value: presentRoutingMode(snapshot.core.routing_mode),
  },
  {
    label: `${m.agent_routing_mode()} · ${m.agent_observed()}`,
    value: presentRoutingMode(snapshot.core.observed_routing_mode),
  },
];

const proxyRows = (snapshot: AgentNetworkSnapshot): AgentStatusRow[] => [
  {
    label: m.agent_desired(),
    value: presentBoolean(snapshot.system_proxy.desired_enabled),
  },
  {
    label: m.agent_observed(),
    value: presentBoolean(snapshot.system_proxy.observed_enabled),
  },
  {
    label: m.agent_proxy_endpoint_match(),
    value: presentBoolean(snapshot.system_proxy.matches_expected_endpoint),
  },
];

const serviceRows = (snapshot: AgentNetworkSnapshot): AgentStatusRow[] => [
  {
    label: m.agent_desired(),
    value: presentBoolean(snapshot.service.desired_enabled),
  },
  {
    label: m.agent_observed(),
    value: presentServiceState(snapshot.service.state),
  },
  {
    label: 'IPC',
    value: presentBoolean(snapshot.service.ipc_connected),
  },
];

const tunRows = (snapshot: AgentNetworkSnapshot): AgentStatusRow[] => [
  {
    label: m.agent_desired(),
    value: presentBoolean(snapshot.tun.desired_enabled),
  },
  {
    label: m.agent_observed(),
    value:
      snapshot.tun.generated_runtime_enabled === null
        ? m.agent_unknown()
        : presentBoolean(snapshot.tun.generated_runtime_enabled),
  },
  {
    label: m.agent_core_state(),
    value: m.agent_unknown(),
  },
];

const profileRows = (snapshot: AgentNetworkSnapshot): AgentStatusRow[] => [
  { label: 'Total', value: snapshot.profiles.total_count },
  { label: 'Active', value: snapshot.profiles.active_count },
  {
    label: m.agent_observed(),
    value: snapshot.profiles.active_references_valid
      ? m.agent_yes()
      : m.agent_no(),
  },
];

const telemetryRows = (snapshot: AgentNetworkSnapshot): AgentStatusRow[] => [
  {
    label: m.agent_core_state(),
    value: presentConnectorState(snapshot.telemetry.state),
  },
  {
    label: m.agent_connections(),
    value: snapshot.telemetry.active_connection_count ?? m.agent_unknown(),
  },
  {
    label: m.agent_upload_speed(),
    value: presentRate(snapshot.telemetry.upload_speed),
  },
  {
    label: m.agent_download_speed(),
    value: presentRate(snapshot.telemetry.download_speed),
  },
];

/** Build six status cards from the public, already-redacted snapshot. */
export function SnapshotSummary({
  snapshot,
}: {
  snapshot: AgentNetworkSnapshot;
}) {
  return (
    <div className="grid gap-4 md:grid-cols-2 xl:grid-cols-3">
      <AgentStatusCard
        icon={<DnsRounded />}
        title={m.agent_core_title()}
        rows={coreRows(snapshot)}
      />
      <AgentStatusCard
        icon={<LanRounded />}
        title={m.agent_system_proxy_title()}
        rows={proxyRows(snapshot)}
      />
      <AgentStatusCard
        icon={<CableRounded />}
        title={m.agent_service_title()}
        rows={serviceRows(snapshot)}
      />
      <AgentStatusCard
        icon={<SecurityRounded />}
        title={m.agent_tun_title()}
        rows={tunRows(snapshot)}
      />
      <AgentStatusCard
        icon={<FolderRounded />}
        title={m.agent_profiles_title()}
        rows={profileRows(snapshot)}
      />
      <AgentStatusCard
        icon={<NetworkCheckRounded />}
        title={m.agent_telemetry_title()}
        rows={telemetryRows(snapshot)}
      />
    </div>
  );
}
