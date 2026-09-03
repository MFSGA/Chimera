import type { CoreState, CoreType, ServiceStatus } from '@chimera/interface';
import * as m from '@/paraglide/messages';

export function getSystemServiceStatusLabel(status?: ServiceStatus): string {
  switch (status) {
    case 'not_installed':
      return m.dashboard_widget_core_service_not_installed();
    case 'running':
      return m.dashboard_widget_core_status_running();
    case 'stopped':
      return m.dashboard_widget_core_status_stopped();
    default:
      return m.common_unknown();
  }
}

export function getAppCoreStatusLabel(status?: CoreState): string {
  if (!status) return m.common_unknown();
  if (status === 'Running') return m.dashboard_widget_core_status_running();

  const stoppedReason = status.Stopped?.trim();
  return stoppedReason
    ? m.dashboard_widget_core_stopped_with_message({ message: stoppedReason })
    : m.dashboard_widget_core_status_stopped();
}

export function getServiceCoreTypeLabel(type?: CoreType | null): string {
  if (!type) return m.common_unknown();
  return typeof type === 'string' ? type : type.clash;
}
