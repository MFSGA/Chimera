import type {
  CoreState,
  CoreStatusInfo,
  ServiceStatus,
} from '@chimera/interface';
import {
  getCoreStatusBadgeMessage,
  getRoutedCoreStatusMessage,
  isRoutedCoreRunning,
} from '@/features/dashboard/core-service-status';
import * as m from '@/paraglide/messages';

type ShortcutCoreSnapshot = {
  actor?: CoreStatusInfo | null;
  legacyCoreState?: CoreState | null;
  serviceStatus?: ServiceStatus | null;
  serviceCoreState?: CoreState | null;
  actorLoading?: boolean;
  actorFailed?: boolean;
  legacyFailed?: boolean;
  serviceFailed?: boolean;
};

/** Ref's header waits for both legacy sources instead of interpreting an
 * unresolved service as "not installed". CoreActor wins when available. */
export function coreShortcutBadgeMessage(
  snapshot: ShortcutCoreSnapshot,
): string {
  if (snapshot.actor) return getRoutedCoreStatusMessage(snapshot.actor);
  if (snapshot.actorLoading) return m.common_loading();
  if (!snapshot.legacyCoreState) {
    return snapshot.actorFailed || snapshot.legacyFailed
      ? m.common_error()
      : m.common_loading();
  }
  if (!snapshot.serviceStatus) {
    return snapshot.serviceFailed ? m.common_error() : m.common_loading();
  }
  return getCoreStatusBadgeMessage({
    coreState: snapshot.legacyCoreState,
    serviceStatus: snapshot.serviceStatus,
    serviceCoreState: snapshot.serviceCoreState ?? undefined,
  });
}

export type CoreShortcutCardPresentation = {
  kind: 'running' | 'stopped' | 'pending' | 'error';
  message: string;
};

/** Only show a running/stopped indicator for a confirmed terminal state.
 * Handoffs, restarts, degraded connections and initial reads are distinct. */
export function coreShortcutCardPresentation(
  snapshot: ShortcutCoreSnapshot,
): CoreShortcutCardPresentation {
  const actor = snapshot.actor;
  if (actor) {
    const message = getRoutedCoreStatusMessage(actor);
    if (actor.connectivity.kind === 'degraded') {
      return { kind: 'error', message };
    }
    if (actor.connectivity.kind === 'shut_down') {
      return { kind: 'stopped', message };
    }
    if (actor.connectivity.kind === 'handing_off' || !actor.state) {
      return { kind: 'pending', message };
    }
    if ('Running' in actor.state) {
      return {
        kind: isRoutedCoreRunning(actor) ? 'running' : 'error',
        message,
      };
    }
    if ('Stopped' in actor.state) return { kind: 'stopped', message };
    return { kind: 'pending', message };
  }

  if (snapshot.actorLoading) {
    return { kind: 'pending', message: m.common_loading() };
  }
  if (!snapshot.legacyCoreState) {
    return {
      kind: snapshot.actorFailed || snapshot.legacyFailed ? 'error' : 'pending',
      message:
        snapshot.actorFailed || snapshot.legacyFailed
          ? m.common_error()
          : m.common_loading(),
    };
  }
  const running = snapshot.legacyCoreState === 'Running';
  return {
    kind: running ? 'running' : 'stopped',
    message: running
      ? m.dashboard_widget_core_status_running()
      : m.dashboard_widget_core_status_stopped(),
  };
}
