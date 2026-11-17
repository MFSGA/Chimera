import assert from 'node:assert/strict';
import test from 'node:test';
import type { CoreStatusInfo } from '@chimera/interface';
import * as m from '@/paraglide/messages';
import { overwriteGetLocale } from '../../paraglide/runtime.js';
import { getRoutedCoreStatusMessage } from './core-service-status';

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
    m.common_loading(),
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
    m.common_loading(),
  );
  assert.equal(
    getRoutedCoreStatusMessage(
      status({ state: { Restarting: { epoch: 2, attempt: 1 } } }),
    ),
    m.settings_clash_core_manager_card_restarting(),
  );
  assert.equal(
    getRoutedCoreStatusMessage(status({ state: null })),
    m.common_loading(),
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
    getRoutedCoreStatusMessage(status({ connectivity: { kind: 'shut_down' } })),
    m.dashboard_widget_core_status_stopped(),
  );
});
