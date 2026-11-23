import assert from 'node:assert/strict';
import test from 'node:test';
import type { CoreStatusInfo } from '@chimera/interface';
import {
  coreShortcutBadgeMessage,
  coreShortcutCardPresentation,
} from '../src/components/widgets/widget-shortcut-status.ts';
import * as m from '../src/paraglide/messages.js';
import { overwriteGetLocale } from '../src/paraglide/runtime.js';

overwriteGetLocale(() => 'en');

function coreActor(override: Partial<CoreStatusInfo> = {}): CoreStatusInfo {
  return {
    controller: null,
    host: 'local',
    connectivity: { kind: 'connected' },
    generation: 1,
    state: { Running: { epoch: 1, pid: 123 } },
    state_changed_at: 10,
    revision: null,
    healthy: true,
    ...override,
  };
}

test('initial CoreActor and service reads never claim a stopped or uninstalled core', () => {
  assert.equal(
    coreShortcutBadgeMessage({
      actorLoading: true,
      legacyCoreState: { Stopped: null },
    }),
    m.common_loading(),
  );
  assert.equal(
    coreShortcutCardPresentation({ actorLoading: true }).kind,
    'pending',
  );
  assert.equal(
    coreShortcutCardPresentation({
      actorLoading: true,
      legacyCoreState: { Stopped: null },
    }).kind,
    'pending',
  );
  assert.equal(
    coreShortcutBadgeMessage({ legacyCoreState: { Stopped: null } }),
    m.common_loading(),
  );
  assert.equal(
    coreShortcutBadgeMessage({
      legacyCoreState: { Stopped: null },
      serviceFailed: true,
    }),
    m.common_error(),
  );
});

test('a resolved legacy service still shows the ref stopped/source diagnostic', () => {
  const message = coreShortcutBadgeMessage({
    legacyCoreState: { Stopped: null },
    serviceStatus: 'not_installed',
  });
  assert.match(message, /not installed/i);
  assert.equal(
    coreShortcutCardPresentation({ legacyCoreState: { Stopped: null } }).kind,
    'stopped',
  );
  assert.equal(
    coreShortcutCardPresentation({ legacyCoreState: 'Running' }).message,
    m.dashboard_widget_core_status_running(),
  );
});

test('authoritative CoreActor running state wins over unresolved legacy and service data', () => {
  assert.deepEqual(coreShortcutCardPresentation({ actor: coreActor() }), {
    kind: 'running',
    message: m.dashboard_widget_core_status_running_by_child_process(),
  });
  assert.equal(
    coreShortcutBadgeMessage({
      actor: coreActor({ host: 'service' }),
      serviceFailed: true,
    }),
    m.dashboard_widget_core_status_running_by_service(),
  );
});

test('starting, restarting, switching and handoffs have no false stopped indicator', () => {
  for (const actor of [
    coreActor({ state: { Starting: { epoch: 2 } } }),
    coreActor({ state: { Restarting: { epoch: 2, attempt: 1 } } }),
    coreActor({ state: { Switching: { from: 1, to: 2 } } }),
    coreActor({ state: { Stopping: { epoch: 2 } } }),
    coreActor({ state: null }),
    coreActor({
      connectivity: { kind: 'handing_off', from: 'local', to: 'service' },
    }),
  ]) {
    assert.equal(coreShortcutCardPresentation({ actor }).kind, 'pending');
  }
});

test('degraded and unhealthy states show errors, while a confirmed stop shows stopped', () => {
  assert.equal(
    coreShortcutCardPresentation({
      actor: coreActor({
        connectivity: {
          kind: 'degraded',
          desired: 'local',
          reason: 'socket failed',
        },
      }),
    }).kind,
    'error',
  );
  assert.equal(
    coreShortcutCardPresentation({ actor: coreActor({ healthy: false }) }).kind,
    'error',
  );
  assert.equal(
    coreShortcutCardPresentation({
      actor: coreActor({ state: { Stopped: { reason: null } } }),
    }).kind,
    'stopped',
  );
  assert.equal(
    coreShortcutCardPresentation({
      actor: coreActor({ connectivity: { kind: 'shut_down' } }),
    }).kind,
    'stopped',
  );
});

test('failed actor reads can use confirmed fallback or report an error', () => {
  assert.equal(
    coreShortcutCardPresentation({ actorFailed: true }).kind,
    'error',
  );
  assert.equal(
    coreShortcutBadgeMessage({ actorFailed: true }),
    m.common_error(),
  );
  assert.equal(
    coreShortcutCardPresentation({
      actorFailed: true,
      legacyCoreState: 'Running',
    }).kind,
    'running',
  );
  assert.equal(
    coreShortcutBadgeMessage({
      actorFailed: true,
      legacyCoreState: 'Running',
      serviceStatus: 'stopped',
    }),
    m.dashboard_widget_core_status_running_by_child_process(),
  );
});
