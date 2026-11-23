import assert from 'node:assert/strict';
import test from 'node:test';
import type { CoreStatusInfo } from '@chimera/interface';
import * as m from '@/paraglide/messages';
import { overwriteGetLocale } from '../../paraglide/runtime.js';
import {
  getRoutedCoreHostLabel,
  getRoutedCoreStatusMessage,
  isCoreLifecycleTransitioning,
  isRoutedCoreRunning,
} from './core-service-status';

overwriteGetLocale(() => 'en');

function status(override: Partial<CoreStatusInfo> = {}): CoreStatusInfo {
  return {
    controller: null,
    host: 'local',
    connectivity: { kind: 'connected' },
    generation: 1,
    state: { Running: { epoch: 1, pid: 345 } },
    state_changed_at: 123,
    revision: null,
    healthy: true,
    ...override,
  };
}

test('ref CoreActor routing states cannot be mislabeled as stopped', () => {
  assert.equal(
    getRoutedCoreStatusMessage(
      status({
        connectivity: { kind: 'handing_off', from: 'local', to: 'service' },
      }),
    ),
    m.dashboard_widget_core_status_switching(),
  );
  assert.equal(
    getRoutedCoreStatusMessage(
      status({
        connectivity: {
          kind: 'degraded',
          desired: 'service',
          reason: 'service lost',
        },
      }),
    ),
    `${m.common_error()}: service lost`,
  );
  assert.equal(
    getRoutedCoreStatusMessage(status({ state: { Starting: { epoch: 2 } } })),
    m.dashboard_widget_core_status_starting(),
  );
  assert.equal(
    getRoutedCoreStatusMessage(
      status({ state: { Restarting: { epoch: 2, attempt: 1 } } }),
    ),
    m.settings_clash_core_manager_card_restarting(),
  );
  assert.equal(
    getRoutedCoreStatusMessage(
      status({ state: { Switching: { from: 1, to: 2 } } }),
    ),
    m.dashboard_widget_core_status_switching(),
  );
  assert.equal(
    getRoutedCoreStatusMessage(status({ state: { Stopping: { epoch: 2 } } })),
    m.dashboard_widget_core_status_stopping(),
  );
  assert.equal(
    getRoutedCoreStatusMessage(status({ state: null })),
    m.common_loading(),
  );
});

test('ref CoreActor only marks a connected, healthy Running core as running', () => {
  assert.equal(isRoutedCoreRunning(status()), true);
  assert.equal(
    isRoutedCoreRunning(
      status({
        connectivity: { kind: 'handing_off', from: 'local', to: 'service' },
      }),
    ),
    false,
  );
  assert.equal(
    isRoutedCoreRunning(
      status({
        connectivity: { kind: 'degraded', desired: 'local', reason: 'lost' },
      }),
    ),
    false,
  );
  assert.equal(
    isRoutedCoreRunning(status({ state: { Starting: { epoch: 4 } } })),
    false,
  );
  assert.equal(isRoutedCoreRunning(status({ state: null })), false);
  assert.equal(isRoutedCoreRunning(status({ healthy: false })), false);
  assert.equal(
    getRoutedCoreStatusMessage(status({ healthy: false })),
    m.common_error(),
  );
});

test('ref CoreActor host display does not report an in-flight destination as committed', () => {
  assert.equal(getRoutedCoreHostLabel(status()), 'normal');
  assert.equal(getRoutedCoreHostLabel(status({ host: 'service' })), 'service');
  assert.equal(
    getRoutedCoreHostLabel(
      status({
        connectivity: {
          kind: 'handing_off',
          from: 'service',
          to: 'local',
        },
      }),
    ),
    'service → normal',
  );
});

test('Core lifecycle controls defer to ref Actor handoffs but keep degraded-host recovery available', () => {
  assert.equal(isCoreLifecycleTransitioning(status()), false);
  assert.equal(isCoreLifecycleTransitioning(undefined), false);
  assert.equal(isCoreLifecycleTransitioning(status({ state: null })), false);
  assert.equal(
    isCoreLifecycleTransitioning(
      status({ state: { Stopped: { reason: null } } }),
    ),
    false,
    'a stopped Core is allowed to be restarted',
  );
  assert.equal(
    isCoreLifecycleTransitioning(
      status({
        connectivity: { kind: 'degraded', desired: 'service', reason: 'lost' },
      }),
    ),
    false,
    'degraded owners must still allow manual recovery',
  );
  assert.equal(
    isCoreLifecycleTransitioning(
      status({
        connectivity: { kind: 'handing_off', from: 'local', to: 'service' },
      }),
    ),
    true,
  );
  assert.equal(
    isCoreLifecycleTransitioning(
      status({ connectivity: { kind: 'shut_down' } }),
    ),
    true,
  );
  assert.equal(
    isCoreLifecycleTransitioning(
      status({ state: { Restarting: { epoch: 2, attempt: 1 } } }),
    ),
    true,
  );
  assert.equal(
    isCoreLifecycleTransitioning(
      status({ state: { Switching: { from: 1, to: 2 } } }),
    ),
    true,
  );
  assert.equal(
    isCoreLifecycleTransitioning(status({ state: { Starting: { epoch: 1 } } })),
    true,
  );
  assert.equal(
    isCoreLifecycleTransitioning(status({ state: { Stopping: { epoch: 1 } } })),
    true,
  );
});

test('ref CoreActor running host and stopped state use legacy localized labels', () => {
  assert.equal(
    getRoutedCoreStatusMessage(status({ host: 'service' })),
    m.dashboard_widget_core_status_running_by_service(),
  );
  assert.equal(
    getRoutedCoreStatusMessage(
      status({ state: { Stopped: { reason: null } } }),
    ),
    m.dashboard_widget_core_status_stopped(),
  );
  assert.equal(
    getRoutedCoreStatusMessage(
      status({ state: { Stopped: { reason: null } }, healthy: false }),
    ),
    m.dashboard_widget_core_status_stopped(),
  );
  assert.equal(
    getRoutedCoreStatusMessage(status({ connectivity: { kind: 'shut_down' } })),
    m.dashboard_widget_core_status_stopped(),
  );
});
