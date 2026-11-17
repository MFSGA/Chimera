import type {
  CoreState,
  CoreStatusInfo,
  RunType,
  ServiceStatus,
} from '@chimera/interface';
import * as m from '@/paraglide/messages';

export function getServiceStatusMessage(status?: ServiceStatus): string {
  switch (status) {
    case 'running':
      return m.dashboard_widget_core_service_running();
    case 'stopped':
      return m.dashboard_widget_core_service_stopped();
    case 'not_installed':
    default:
      return m.dashboard_widget_core_service_not_installed();
  }
}

/** Ref CoreActor status takes precedence when available. Transitional and
 * degraded owners are NOT collapsed into the legacy "Stopped" state. */
export function getRoutedCoreStatusMessage(status: CoreStatusInfo): string {
  switch (status.connectivity.kind) {
    case 'degraded':
      return status.connectivity.reason
        ? `${m.common_error()}: ${status.connectivity.reason}`
        : m.common_error();
    case 'handing_off':
      return m.common_loading();
    case 'shut_down':
      return m.dashboard_widget_core_status_stopped();
    case 'connected':
      break;
  }

  const state = status.state;
  if (!state) return m.common_loading();
  if ('Running' in state) {
    return getRunningCoreMessage({
      coreType: status.host === 'service' ? 'service' : 'normal',
    });
  }
  if ('Stopped' in state && state.Stopped) {
    const reason = state.Stopped.reason;
    return reason
      ? m.dashboard_widget_core_stopped_with_message({ message: reason })
      : m.dashboard_widget_core_status_stopped();
  }
  if ('Restarting' in state) {
    return m.settings_clash_core_manager_card_restarting();
  }
  // Starting, Switching and Stopping are transient runtime states.
  return m.common_loading();
}

export function getStoppedCoreReason(state?: CoreState): string | null {
  if (!state || state === 'Running') return null;
  return state.Stopped || null;
}

type RunningCoreSource = {
  coreType?: RunType;
  serviceCoreState?: CoreState;
};

export function getRunningCoreMessage({
  coreType,
  serviceCoreState,
}: RunningCoreSource): string {
  if (
    serviceCoreState === 'Running' ||
    (coreType !== undefined && coreType !== 'normal')
  ) {
    return m.dashboard_widget_core_status_running_by_service();
  }

  return m.dashboard_widget_core_status_running_by_child_process();
}

type CoreStatusBadgeMessage = {
  coreState?: CoreState;
  serviceStatus?: ServiceStatus;
  serviceCoreState?: CoreState;
};

export function getCoreStatusBadgeMessage({
  coreState,
  serviceStatus,
  serviceCoreState,
}: CoreStatusBadgeMessage): string {
  if (coreState === 'Running') {
    return getRunningCoreMessage({ serviceCoreState });
  }

  const stoppedReason = getStoppedCoreReason(coreState);
  const serviceMessage = getServiceStatusMessage(serviceStatus);

  let stoppedMessage = m.dashboard_widget_core_stopped_unknown();

  if (serviceStatus === 'running') {
    stoppedMessage = stoppedReason
      ? m.dashboard_widget_core_stopped_by_service_with_message({
          message: stoppedReason,
        })
      : m.dashboard_widget_core_stopped_by_service_unknown();
  }

  if (stoppedReason) {
    stoppedMessage = m.dashboard_widget_core_stopped_with_message({
      message: stoppedReason,
    });
  }

  return `${stoppedMessage} ${serviceMessage}`;
}
