import type {
  AgentFindingCode,
  AgentHealth,
  AgentImpact,
  AgentProbeCode,
  AgentRoutingMode,
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
};

const probeMessages: Record<AgentProbeCode, () => string> = {
  core_config_unavailable: m.agent_probe_core_config_unavailable,
  system_proxy_unavailable: m.agent_probe_system_proxy_unavailable,
  service_status_unavailable: m.agent_probe_service_status_unavailable,
  service_status_timeout: m.agent_probe_service_status_timeout,
  telemetry_unavailable: m.agent_probe_telemetry_unavailable,
};

const impactMessages: Record<AgentImpact, () => string> = {
  existing_connections_may_change:
    m.agent_impact_existing_connections_may_change,
  traffic_may_bypass_proxy: m.agent_impact_traffic_may_bypass_proxy,
  all_traffic_uses_proxy: m.agent_impact_all_traffic_uses_proxy,
  restore_rule_routing: m.agent_impact_restore_rule_routing,
  host_system_proxy_disabled: m.agent_impact_host_system_proxy_disabled,
};

const routingModeMessages: Record<AgentRoutingMode, () => string> = {
  rule: m.agent_mode_rule,
  global: m.agent_mode_global,
  direct: m.agent_mode_direct,
};

export const presentHealth = (health: AgentHealth) => healthMessages[health]();

export const presentFinding = (code: AgentFindingCode) =>
  findingMessages[code]();

export const presentProbe = (code: AgentProbeCode) => probeMessages[code]();

export const presentImpact = (impact: AgentImpact) => impactMessages[impact]();

export const presentRoutingMode = (mode: AgentRoutingMode | null) =>
  mode ? routingModeMessages[mode]() : m.agent_unknown();

export const presentBoolean = (value: boolean | null) => {
  if (value === null) return m.agent_unknown();
  return value ? m.agent_enabled() : m.agent_disabled();
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
