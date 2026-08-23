import type { AgentNetworkSnapshot } from '@chimera/interface';
import {
  AdminPanelSettingsRounded,
  CableRounded,
  DnsRounded,
  FolderRounded,
  LanRounded,
  NetworkCheckRounded,
  PublicRounded,
  SecurityRounded,
} from '@mui/icons-material';
import * as m from '@/paraglide/messages';
import {
  presentBoolean,
  presentConnectorState,
  presentCoreState,
  presentHealth,
  presentHostConnectivityReason,
  presentHostConnectivityStatus,
  presentNetworkInterfaceKind,
  presentPlatformReadinessReason,
  presentProcessPrivilege,
  presentRate,
  presentRoutingMode,
  presentServiceState,
  presentSystemDnsVerification,
  presentTunPermissionReadiness,
  presentTunVerification,
  presentYesNo,
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
    label: m.agent_generated_config(),
    value:
      snapshot.tun.generated_runtime_enabled === null
        ? m.agent_unknown()
        : presentBoolean(snapshot.tun.generated_runtime_enabled),
  },
  {
    label: m.agent_core_state(),
    value:
      snapshot.tun.observed_enabled === null
        ? m.agent_unknown()
        : presentBoolean(snapshot.tun.observed_enabled),
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

const connectivityRows = (snapshot: AgentNetworkSnapshot): AgentStatusRow[] => {
  const connectivity = snapshot.connectivity;
  const status = presentHostConnectivityStatus(connectivity.status);
  const reasons = connectivity.reasons.map(presentHostConnectivityReason);

  return [
    { label: m.agent_connectivity_status(), value: status.label },
    { label: m.agent_health_title(), value: presentHealth(status.health) },
    {
      label: m.agent_connectivity_interface(),
      value: presentNetworkInterfaceKind(connectivity.active_interface_kind),
    },
    {
      label: m.agent_connectivity_link(),
      value: presentYesNo(connectivity.link_up),
    },
    {
      label: `${m.agent_connectivity_ipv4()} · ${m.agent_connectivity_usable_ip()}`,
      value: presentYesNo(connectivity.ipv4.usable_ip),
    },
    {
      label: `${m.agent_connectivity_ipv4()} · ${m.agent_connectivity_default_route()}`,
      value: presentYesNo(connectivity.ipv4.default_route),
    },
    {
      label: `${m.agent_connectivity_ipv4()} · ${m.agent_connectivity_internet()}`,
      value: presentYesNo(connectivity.ipv4.internet_reachable),
    },
    {
      label: `${m.agent_connectivity_ipv6()} · ${m.agent_connectivity_usable_ip()}`,
      value: presentYesNo(connectivity.ipv6.usable_ip),
    },
    {
      label: `${m.agent_connectivity_ipv6()} · ${m.agent_connectivity_default_route()}`,
      value: presentYesNo(connectivity.ipv6.default_route),
    },
    {
      label: `${m.agent_connectivity_ipv6()} · ${m.agent_connectivity_internet()}`,
      value: presentYesNo(connectivity.ipv6.internet_reachable),
    },
    {
      label: m.agent_connectivity_dns_configured(),
      value: presentYesNo(connectivity.dns_configured),
    },
    {
      label: m.agent_connectivity_dns_resolves(),
      value: presentYesNo(connectivity.dns_resolves),
    },
    {
      label: m.agent_connectivity_captive_portal(),
      value: presentYesNo(connectivity.captive_portal_suspected),
    },
    {
      label: m.agent_connectivity_reasons(),
      value: reasons.length
        ? reasons.join(' · ')
        : m.agent_connectivity_reason_none(),
    },
  ];
};

const readinessRows = (snapshot: AgentNetworkSnapshot): AgentStatusRow[] => {
  const readiness = snapshot.platform_readiness;
  const reasons = readiness.reasons.map(presentPlatformReadinessReason);

  return [
    {
      label: m.agent_readiness_process_privilege(),
      value: presentProcessPrivilege(readiness.process_privilege).label,
    },
    {
      label: m.agent_readiness_service_mode_available(),
      value: presentYesNo(readiness.service_mode_available),
    },
    {
      label: m.agent_readiness_tun_permission(),
      value: presentTunPermissionReadiness(readiness.tun_permission).label,
    },
    {
      label: m.agent_readiness_tun_verification(),
      value: presentTunVerification(readiness.tun_verification).label,
    },
    {
      label: m.agent_readiness_system_dns(),
      value: presentSystemDnsVerification(readiness.system_dns_verification)
        .label,
    },
    {
      label: m.agent_readiness_reasons(),
      value: reasons.length
        ? reasons.join(' · ')
        : m.agent_readiness_reason_none(),
    },
  ];
};

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

/** Build privacy-safe status cards from the public, already-redacted snapshot. */
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
      <div id="agent-host-connectivity-card" tabIndex={-1}>
        <AgentStatusCard
          icon={<PublicRounded />}
          title={m.agent_connectivity_title()}
          rows={connectivityRows(snapshot)}
        />
      </div>
      <AgentStatusCard
        icon={<AdminPanelSettingsRounded />}
        title={m.agent_readiness_title()}
        rows={readinessRows(snapshot)}
      />
      <AgentStatusCard
        icon={<NetworkCheckRounded />}
        title={m.agent_telemetry_title()}
        rows={telemetryRows(snapshot)}
      />
    </div>
  );
}
