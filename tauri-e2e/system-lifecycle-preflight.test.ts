import assert from 'node:assert/strict';
import test from 'node:test';
import { assertSystemLifecyclePreflight } from './system-lifecycle-preflight.ts';

test('system lifecycle preflight accepts only an owned clean elevated Windows runner', () => {
  assert.doesNotThrow(() =>
    assertSystemLifecyclePreflight({
      platform: 'win32',
      optedIn: true,
      elevated: true,
      serviceStatus: 'not_installed',
    }),
  );
});

test('system lifecycle preflight rejects unsafe host ownership states', () => {
  const base = {
    platform: 'win32' as NodeJS.Platform,
    optedIn: true,
    elevated: true,
    serviceStatus: 'not_installed',
  };

  assert.throws(
    () => assertSystemLifecyclePreflight({ ...base, optedIn: false }),
    /CHIMERA_E2E_SYSTEM_LIFECYCLE=1/,
  );
  assert.throws(
    () =>
      assertSystemLifecyclePreflight({
        ...base,
        platform: 'linux',
      }),
    /only supported on Windows/,
  );
  assert.throws(
    () => assertSystemLifecyclePreflight({ ...base, elevated: false }),
    /elevated Windows runner/,
  );
  assert.throws(
    () =>
      assertSystemLifecyclePreflight({
        ...base,
        serviceStatus: 'running',
      }),
    /refuses to take ownership/,
  );
});
