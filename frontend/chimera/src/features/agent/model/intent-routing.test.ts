import assert from 'node:assert/strict';
import test from 'node:test';
import type { AgentIntent } from '@chimera/interface';
import { routeAgentIntent } from './intent-routing';

test('diagnostics are read-only, and every network change becomes a proposal', () => {
  assert.deepEqual(routeAgentIntent({ intent: 'diagnose' }), {
    kind: 'diagnose',
  });
  const requests: Array<[AgentIntent, unknown]> = [
    [
      { intent: 'set_tun_enabled', enabled: true },
      { action: 'set_tun_enabled', enabled: true },
    ],
    [
      { intent: 'set_tun_enabled', enabled: false },
      { action: 'set_tun_enabled', enabled: false },
    ],
    [
      { intent: 'set_system_proxy_enabled', enabled: true },
      { action: 'set_system_proxy_enabled', enabled: true },
    ],
    [
      { intent: 'set_system_proxy_enabled', enabled: false },
      { action: 'set_system_proxy_enabled', enabled: false },
    ],
    [
      { intent: 'set_routing_mode', mode: 'rule' },
      { action: 'set_routing_mode', mode: 'rule' },
    ],
    [
      { intent: 'set_routing_mode', mode: 'direct' },
      { action: 'set_routing_mode', mode: 'direct' },
    ],
    [
      { intent: 'set_routing_mode', mode: 'global' },
      { action: 'set_routing_mode', mode: 'global' },
    ],
    [
      { intent: 'disable_stale_system_proxy' },
      { action: 'disable_stale_system_proxy' },
    ],
  ];
  for (const [intent, action] of requests) {
    assert.deepEqual(routeAgentIntent(intent), { kind: 'proposal', action });
  }
});
