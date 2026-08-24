import type {
  AgentActionKind,
  AgentAuditOutcome,
  AgentCommandError,
  AgentFindingCode,
  AgentHealth,
  AgentHostConnectivityReason,
  AgentHostConnectivityStatus,
  AgentImpact,
  AgentNetworkInterfaceKind,
  AgentPlatformReadinessReason,
  AgentProbeCode,
  AgentProcessPrivilegeStatus,
  AgentRoutingMode,
  AgentStateValue,
  AgentSystemDnsVerificationStatus,
  AgentTunPermissionReadiness,
  AgentTunVerificationStatus,
} from '@chimera/interface';
import * as m from '@/paraglide/messages';

const healthMessages: Record<AgentHealth, () => string> = {
  healthy: m.agent_health_healthy,
  warning: m.agent_health_warning,
  critical: m.agent_health_critical,
  degraded: m.agent_health_degraded,
};

const findingMessages: Record<AgentFindingCode, () => string> = {
  weak_controller_secret: m.agent_finding_weak_controller_secret,
  system_proxy_without_running_core:
    m.agent_finding_system_proxy_without_running_core,
  system_proxy_endpoint_mismatch:
    m.agent_finding_system_proxy_endpoint_mismatch,
  runtime_config_missing: m.agent_finding_runtime_config_missing,
  active_profile_missing: m.agent_finding_active_profile_missing,
  service_mode_inconsistent: m.agent_finding_service_mode_inconsistent,
  clash_connector_disconnected: m.agent_finding_clash_connector_disconnected,
  tun_runtime_mismatch: m.agent_finding_tun_runtime_mismatch,
  recent_core_errors: m.agent_finding_recent_core_errors,
  host_link_disconnected: m.agent_finding_host_link_disconnected,
  host_address_unavailable: m.agent_finding_host_address_unavailable,
  host_default_route_unavailable:
    m.agent_finding_host_default_route_unavailable,
  host_dns_unavailable: m.agent_finding_host_dns_unavailable,
  host_captive_portal_suspected: m.agent_finding_host_captive_portal_suspected,
  host_internet_unreachable: m.agent_finding_host_internet_unreachable,
  host_ipv4_only: m.agent_finding_host_ipv4_only,
  host_ipv6_only: m.agent_finding_host_ipv6_only,
  tun_permission_required: m.agent_finding_tun_permission_required,
  tun_system_dns_unverified: m.agent_finding_tun_system_dns_unverified,
};

const probeMessages: Record<AgentProbeCode, () => string> = {
  core_status_unavailable: m.agent_probe_core_status_unavailable,
  core_status_timeout: m.agent_probe_core_status_timeout,
  core_config_unavailable: m.agent_probe_core_config_unavailable,
  tun_status_unavailable: m.agent_probe_tun_status_unavailable,
  system_proxy_unavailable: m.agent_probe_system_proxy_unavailable,
  service_status_unavailable: m.agent_probe_service_status_unavailable,
  service_status_timeout: m.agent_probe_service_status_timeout,
  telemetry_unavailable: m.agent_probe_telemetry_unavailable,
  host_connectivity_unavailable: m.agent_probe_host_connectivity_unavailable,
  platform_readiness_unavailable: m.agent_probe_platform_readiness_unavailable,
};

const impactMessages: Record<AgentImpact, () => string> = {
  existing_connections_may_change:
    m.agent_impact_existing_connections_may_change,
  core_may_restart: m.agent_impact_core_may_restart,
  host_dns_may_change: m.agent_impact_host_dns_may_change,
  admin_permission_may_be_required:
    m.agent_impact_admin_permission_may_be_required,
  traffic_may_bypass_proxy: m.agent_impact_traffic_may_bypass_proxy,
  all_traffic_uses_proxy: m.agent_impact_all_traffic_uses_proxy,
  restore_rule_routing: m.agent_impact_restore_rule_routing,
  host_system_proxy_enabled: m.agent_impact_host_system_proxy_enabled,
  host_system_proxy_disabled: m.agent_impact_host_system_proxy_disabled,
  host_system_proxy_endpoint_changed:
    m.agent_impact_host_system_proxy_endpoint_changed,
  service_availability_may_change:
    m.agent_impact_service_availability_may_change,
  telemetry_may_be_unavailable: m.agent_impact_telemetry_may_be_unavailable,
};

const connectivityStatusMessages: Record<
  AgentHostConnectivityStatus,
  () => string
> = {
  online_dual_stack: m.agent_connectivity_status_online_dual_stack,
  online_ipv4_only: m.agent_connectivity_status_online_ipv4_only,
  online_ipv6_only: m.agent_connectivity_status_online_ipv6_only,
  link_disconnected: m.agent_connectivity_status_link_disconnected,
  address_unavailable: m.agent_connectivity_status_address_unavailable,
  default_route_unavailable:
    m.agent_connectivity_status_default_route_unavailable,
  dns_unavailable: m.agent_connectivity_status_dns_unavailable,
  captive_portal_suspected:
    m.agent_connectivity_status_captive_portal_suspected,
  internet_unreachable: m.agent_connectivity_status_internet_unreachable,
  indeterminate: m.agent_connectivity_status_indeterminate,
};

const connectivityHealth: Record<AgentHostConnectivityStatus, AgentHealth> = {
  online_dual_stack: 'healthy',
  online_ipv4_only: 'healthy',
  online_ipv6_only: 'healthy',
  link_disconnected: 'critical',
  address_unavailable: 'critical',
  default_route_unavailable: 'critical',
  dns_unavailable: 'critical',
  captive_portal_suspected: 'warning',
  internet_unreachable: 'critical',
  indeterminate: 'degraded',
};

const connectivityReasonMessages: Record<
  AgentHostConnectivityReason,
  () => string
> = {
  probe_unavailable: m.agent_connectivity_reason_probe_unavailable,
  no_active_interface: m.agent_connectivity_reason_no_active_interface,
  wireless_disconnected: m.agent_connectivity_reason_wireless_disconnected,
  ethernet_disconnected: m.agent_connectivity_reason_ethernet_disconnected,
  no_usable_ipv4_address: m.agent_connectivity_reason_no_usable_ipv4_address,
  no_usable_ipv6_address: m.agent_connectivity_reason_no_usable_ipv6_address,
  no_ipv4_default_route: m.agent_connectivity_reason_no_ipv4_default_route,
  no_ipv6_default_route: m.agent_connectivity_reason_no_ipv6_default_route,
  dns_not_configured: m.agent_connectivity_reason_dns_not_configured,
  dns_resolution_failed: m.agent_connectivity_reason_dns_resolution_failed,
  ipv4_internet_unreachable:
    m.agent_connectivity_reason_ipv4_internet_unreachable,
  ipv6_internet_unreachable:
    m.agent_connectivity_reason_ipv6_internet_unreachable,
  captive_portal_suspected:
    m.agent_connectivity_reason_captive_portal_suspected,
};

const platformReadinessReasonMessages: Record<
  AgentPlatformReadinessReason,
  () => string
> = {
  privilege_probe_unavailable:
    m.agent_readiness_reason_privilege_probe_unavailable,
  elevated_process: m.agent_readiness_reason_elevated_process,
  service_mode_active: m.agent_readiness_reason_service_mode_active,
  service_mode_available: m.agent_readiness_reason_service_mode_available,
  permission_required: m.agent_readiness_reason_permission_required,
  tun_state_unavailable: m.agent_readiness_reason_tun_state_unavailable,
  tun_state_inconsistent: m.agent_readiness_reason_tun_state_inconsistent,
  system_dns_not_configured: m.agent_readiness_reason_system_dns_not_configured,
  system_dns_resolution_failed:
    m.agent_readiness_reason_system_dns_resolution_failed,
  system_dns_unavailable: m.agent_readiness_reason_system_dns_unavailable,
};

const processPrivilegeMessages: Record<
  AgentProcessPrivilegeStatus,
  () => string
> = {
  elevated: m.agent_readiness_privilege_elevated,
  standard: m.agent_readiness_privilege_standard,
  unknown: m.agent_readiness_privilege_unknown,
};

const processPrivilegeHealth: Record<AgentProcessPrivilegeStatus, AgentHealth> =
  {
    elevated: 'healthy',
    standard: 'warning',
    unknown: 'degraded',
  };

const tunPermissionMessages: Record<AgentTunPermissionReadiness, () => string> =
  {
    not_required: m.agent_readiness_not_required,
    satisfied: m.agent_readiness_tun_permission_satisfied,
    service_alternative_available:
      m.agent_readiness_tun_permission_service_alternative,
    required: m.agent_readiness_tun_permission_required,
    indeterminate: m.agent_readiness_indeterminate,
  };

const tunPermissionHealth: Record<AgentTunPermissionReadiness, AgentHealth> = {
  not_required: 'healthy',
  satisfied: 'healthy',
  service_alternative_available: 'healthy',
  required: 'critical',
  indeterminate: 'degraded',
};

const tunVerificationMessages: Record<
  AgentTunVerificationStatus,
  () => string
> = {
  not_requested: m.agent_readiness_tun_not_requested,
  verified: m.agent_readiness_tun_verified,
  inconsistent: m.agent_readiness_tun_inconsistent,
  unavailable: m.agent_readiness_unavailable,
};

const tunVerificationHealth: Record<AgentTunVerificationStatus, AgentHealth> = {
  not_requested: 'healthy',
  verified: 'healthy',
  inconsistent: 'critical',
  unavailable: 'degraded',
};

const systemDnsVerificationMessages: Record<
  AgentSystemDnsVerificationStatus,
  () => string
> = {
  not_required: m.agent_readiness_not_required,
  verified: m.agent_readiness_system_dns_verified,
  not_configured: m.agent_readiness_system_dns_not_configured,
  resolution_failed: m.agent_readiness_system_dns_resolution_failed,
  unavailable: m.agent_readiness_unavailable,
};

const systemDnsVerificationHealth: Record<
  AgentSystemDnsVerificationStatus,
  AgentHealth
> = {
  not_required: 'healthy',
  verified: 'healthy',
  not_configured: 'critical',
  resolution_failed: 'critical',
  unavailable: 'degraded',
};

const networkInterfaceMessages: Record<
  AgentNetworkInterfaceKind,
  () => string
> = {
  wireless: m.agent_connectivity_interface_wireless,
  ethernet: m.agent_connectivity_interface_ethernet,
  multiple: m.agent_connectivity_interface_multiple,
  other: m.agent_connectivity_interface_other,
  none: m.agent_connectivity_interface_none,
  unknown: m.agent_connectivity_interface_unknown,
};

const routingModeMessages: Record<AgentRoutingMode, () => string> = {
  rule: m.agent_mode_rule,
  global: m.agent_mode_global,
  direct: m.agent_mode_direct,
};

const stateValueMessages: Record<AgentStateValue, () => string> = {
  rule: m.agent_mode_rule,
  global: m.agent_mode_global,
  direct: m.agent_mode_direct,
  running: m.agent_state_running,
  stopped: m.agent_state_stopped,
  restarted: m.agent_restart_core,
  connected: m.agent_state_connected,
  disconnected: m.agent_state_disconnected,
  unexpected: m.agent_unknown,
  expected_loopback_endpoint: m.agent_proxy_endpoint_match,
  enabled: m.agent_enabled,
  disabled: m.agent_disabled,
};

const actionKindMessages: Record<AgentActionKind, () => string> = {
  set_routing_mode: m.agent_set_mode,
  set_tun_enabled: m.agent_tun_title,
  set_system_proxy_enabled: m.agent_system_proxy_title,
  set_service_mode: m.settings_system_proxy_service_mode_label,
  start_core: m.agent_start_core,
  restart_core: m.agent_restart_core,
  reconnect_telemetry: m.agent_reconnect_telemetry,
  start_service: m.agent_start_service,
  stop_service: m.agent_stop_service,
  restart_service: m.agent_restart_service,
  repair_system_proxy_endpoint: m.agent_repair_proxy_endpoint,
  disable_stale_system_proxy: m.agent_disable_stale_proxy,
};

const agentErrorMessages: Record<AgentCommandError, () => string> = {
  agent_action_not_available: m.agent_error_action_not_available,
  agent_proposal_not_found: m.agent_error_proposal_not_found,
  agent_proposal_expired: m.agent_error_proposal_expired,
  agent_proposal_digest_mismatch: m.agent_error_proposal_digest_mismatch,
  agent_network_state_changed: m.agent_error_network_state_changed,
  agent_proposal_rate_limited: m.agent_error_proposal_rate_limited,
  agent_proposal_limit_reached: m.agent_error_proposal_limit_reached,
  agent_confirmation_declined: m.agent_error_confirmation_declined,
  agent_action_failed: m.agent_error_action_failed,
  agent_action_partially_applied: m.agent_error_action_partially_applied,
  agent_action_verification_failed: m.agent_error_action_verification_failed,
  agent_bridge_start_failed: m.agent_error_bridge_start_failed,
  agent_history_clear_failed: m.agent_error_history_clear_failed,
};

const auditOutcomeMessages: Record<AgentAuditOutcome, () => string> = {
  proposed: m.agent_history_outcome_proposed,
  verified: m.agent_action_success,
  action_not_available: m.agent_error_action_not_available,
  proposal_not_found: m.agent_error_proposal_not_found,
  proposal_expired: m.agent_error_proposal_expired,
  digest_mismatch: m.agent_error_proposal_digest_mismatch,
  state_changed: m.agent_error_network_state_changed,
  rate_limited: m.agent_error_proposal_rate_limited,
  limit_reached: m.agent_error_proposal_limit_reached,
  confirmation_declined: m.agent_error_confirmation_declined,
  action_failed: m.agent_error_action_failed,
  partial_apply: m.agent_error_action_partially_applied,
  verification_failed: m.agent_error_action_verification_failed,
  bridge_start_failed: m.agent_error_bridge_start_failed,
  history_clear_failed: m.agent_error_history_clear_failed,
};

const isAgentCommandError = (error: unknown): error is AgentCommandError =>
  typeof error === 'string' && Object.hasOwn(agentErrorMessages, error);

export const presentAgentError = (error: unknown) =>
  isAgentCommandError(error)
    ? agentErrorMessages[error]()
    : m.agent_error_unknown();

export const presentActionKind = (action: AgentActionKind) =>
  actionKindMessages[action]();

export const presentAuditOutcome = (outcome: AgentAuditOutcome) =>
  auditOutcomeMessages[outcome]();

export const presentHealth = (health: AgentHealth) => healthMessages[health]();

export const presentHostConnectivityStatus = (
  status: AgentHostConnectivityStatus,
) => ({
  label: connectivityStatusMessages[status](),
  health: connectivityHealth[status],
});

export const presentHostConnectivityReason = (
  reason: AgentHostConnectivityReason,
) => connectivityReasonMessages[reason]();

export const presentNetworkInterfaceKind = (kind: AgentNetworkInterfaceKind) =>
  networkInterfaceMessages[kind]();

export const presentPlatformReadinessReason = (
  reason: AgentPlatformReadinessReason,
) => platformReadinessReasonMessages[reason]();

export const presentProcessPrivilege = (
  status: AgentProcessPrivilegeStatus,
) => ({
  label: processPrivilegeMessages[status](),
  health: processPrivilegeHealth[status],
});

export const presentTunPermissionReadiness = (
  status: AgentTunPermissionReadiness,
) => ({
  label: tunPermissionMessages[status](),
  health: tunPermissionHealth[status],
});

export const presentTunVerification = (status: AgentTunVerificationStatus) => ({
  label: tunVerificationMessages[status](),
  health: tunVerificationHealth[status],
});

export const presentSystemDnsVerification = (
  status: AgentSystemDnsVerificationStatus,
) => ({
  label: systemDnsVerificationMessages[status](),
  health: systemDnsVerificationHealth[status],
});

export const presentFinding = (code: AgentFindingCode) =>
  findingMessages[code]();

export const presentProbe = (code: AgentProbeCode) => probeMessages[code]();

export const presentImpact = (impact: AgentImpact) => impactMessages[impact]();

export const presentRoutingMode = (mode: AgentRoutingMode | null) =>
  mode ? routingModeMessages[mode]() : m.agent_unknown();

export const presentStateValue = (value: AgentStateValue) =>
  stateValueMessages[value]();

export const presentBoolean = (value: boolean | null) => {
  if (value === null) return m.agent_unknown();
  return value ? m.agent_enabled() : m.agent_disabled();
};

export const presentYesNo = (value: boolean | null) => {
  if (value === null) return m.agent_unknown();
  return value ? m.agent_yes() : m.agent_no();
};

export const presentCoreState = (state: 'running' | 'stopped' | 'unknown') => {
  if (state === 'running') return m.agent_core_running();
  if (state === 'stopped') return m.agent_core_stopped();
  return m.agent_core_unknown();
};

export const presentServiceState = (
  state: 'not_installed' | 'stopped' | 'running' | 'unknown',
) => {
  if (state === 'not_installed') return m.agent_service_not_installed();
  if (state === 'running') return m.agent_state_running();
  if (state === 'stopped') return m.agent_state_stopped();
  return m.agent_unknown();
};

export const presentConnectorState = (
  state: 'disconnected' | 'connecting' | 'connected' | 'unknown',
) => {
  if (state === 'connected') return m.agent_state_connected();
  if (state === 'connecting') return m.agent_state_connecting();
  if (state === 'disconnected') return m.agent_state_disconnected();
  return m.agent_unknown();
};

export const presentRate = (value: number | null) => {
  if (value === null) return m.agent_unknown();
  return `${new Intl.NumberFormat().format(value)} B/s`;
};
