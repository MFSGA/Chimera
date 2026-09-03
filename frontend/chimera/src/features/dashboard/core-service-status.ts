import type { CoreState, RunType, ServiceStatus } from '@chimera/interface';
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
