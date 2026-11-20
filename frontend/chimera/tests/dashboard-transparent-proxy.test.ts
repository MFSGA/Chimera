import assert from 'node:assert/strict';
import test from 'node:test';
import { getProxyStatus } from '../src/features/dashboard/proxy-status.ts';
import {
  canToggleTransparentProxy,
  isTransparentProxyApplied,
  transparentProxyNextPatch,
  type TransparentProxyAdmission,
} from '../src/features/system-proxy/transparent-proxy-policy.ts';

const ready: TransparentProxyAdmission = {
  supportedHost: true,
  supportedCore: true,
  serviceModeEnabled: true,
  statusSupported: true,
  tunEnabled: false,
  configReady: true,
  mode: 'disabled',
};

test('transparent capture defaults fail closed when prerequisites are absent', () => {
  assert.equal(canToggleTransparentProxy(ready), true);
  for (const patch of [
    { supportedHost: false },
    { supportedCore: false },
    { serviceModeEnabled: false },
    { statusSupported: false },
    { tunEnabled: true },
    { configReady: false },
  ]) {
    assert.equal(canToggleTransparentProxy({ ...ready, ...patch }), false);
  }
});

test('previously enabled capture remains possible to disable when service or TUN state changes', () => {
  assert.equal(
    canToggleTransparentProxy({
      ...ready,
      mode: 'redir',
      serviceModeEnabled: false,
      statusSupported: false,
      tunEnabled: true,
    }),
    true,
  );
  assert.equal(
    canToggleTransparentProxy({
      ...ready,
      mode: 'tproxy',
      supportedHost: false,
    }),
    false,
  );
});

test('enable sets a typed TProxy port only if it is not configured; disable does not overwrite ports', () => {
  assert.deepEqual(transparentProxyNextPatch('disabled', false), {
    transparent_proxy: { mode: 'tproxy' },
    tproxy_port: { kind: 'fixed', start_port: 7894 },
  });
  assert.deepEqual(transparentProxyNextPatch('disabled', true), {
    transparent_proxy: { mode: 'tproxy' },
  });
  assert.deepEqual(transparentProxyNextPatch('redir', false), {
    transparent_proxy: { mode: 'disabled' },
  });
  assert.deepEqual(transparentProxyNextPatch('tproxy', false), {
    transparent_proxy: { mode: 'disabled' },
  });
});

test('proxy title prefers actual TUN/transparent state over less authoritative system settings', () => {
  const base = {
    enableSystemProxy: true,
    systemProxyEnabled: true,
    systemProxyServer: '127.0.0.1:7890',
    mixedPort: 7890,
    enableTunMode: false,
    transparentProxyActive: false,
  };
  assert.equal(getProxyStatus(base), 'system');
  assert.equal(
    getProxyStatus({ ...base, transparentProxyActive: true }),
    'transparent',
  );
  assert.equal(
    getProxyStatus({
      ...base,
      transparentProxyActive: true,
      enableTunMode: true,
    }),
    'tun',
  );
  assert.equal(
    getProxyStatus({ ...base, enableSystemProxy: false }),
    'disabled',
  );
  assert.equal(getProxyStatus({ ...base, mixedPort: 7891 }), 'occupied');
});

test('configured capture does not claim active until the same mode is confirmed by the service', () => {
  const base = {
    visible: true,
    serviceModeEnabled: true,
    configuredMode: 'tproxy' as const,
    status: { supported: true, active: true, mode: 'tproxy', error: null },
  };
  assert.equal(isTransparentProxyApplied(base), true);
  for (const patch of [
    { visible: false },
    { serviceModeEnabled: false },
    { configuredMode: 'disabled' as const },
    { configuredMode: 'redir' as const },
    { status: { supported: true, active: false, mode: 'tproxy', error: null } },
    { status: { supported: true, active: true, mode: 'redir', error: null } },
    { status: { supported: false, active: true, mode: 'tproxy', error: null } },
    {
      status: {
        supported: true,
        active: true,
        mode: 'tproxy',
        error: 'rules not installed',
      },
    },
    { status: null },
    { statusFresh: false },
    { isSyncing: true },
  ]) {
    assert.equal(isTransparentProxyApplied({ ...base, ...patch }), false);
  }
});

test('a previous enabled snapshot cannot mark newly disabled capture as applied', () => {
  const stale = { supported: true, active: true, mode: 'tproxy', error: null };
  assert.equal(
    isTransparentProxyApplied({
      visible: true,
      serviceModeEnabled: true,
      configuredMode: 'disabled',
      status: stale,
    }),
    false,
  );
  assert.equal(
    isTransparentProxyApplied({
      visible: true,
      serviceModeEnabled: false,
      configuredMode: 'tproxy',
      status: stale,
    }),
    false,
  );
});
