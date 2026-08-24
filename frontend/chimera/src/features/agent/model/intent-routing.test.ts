import assert from 'node:assert/strict';
import test from 'node:test';
import type {
  AgentExecuteReadOnlyIntentResult,
  AgentIntent,
} from '@chimera/interface';
import {
  executeReadOnlyAgentIntent,
  resolveExecutedReadOnlyIntent,
  routeAgentIntent,
} from './intent-routing';

test('host connectivity remains read only and never becomes a proposal', () => {
  assert.deepEqual(routeAgentIntent({ intent: 'host_connectivity' }), {
    kind: 'host_connectivity',
  });
  assert.deepEqual(routeAgentIntent({ intent: 'diagnose' }), {
    kind: 'diagnose',
  });
});

test('resolved read-only intents execute without a second user action', async () => {
  const calls: string[] = [];
  const handlers = {
    diagnose: () => calls.push('diagnose'),
    hostConnectivity: () => calls.push('host_connectivity'),
  };

  assert.equal(
    await executeReadOnlyAgentIntent({ intent: 'diagnose' }, handlers),
    true,
  );
  assert.equal(
    await executeReadOnlyAgentIntent({ intent: 'host_connectivity' }, handlers),
    true,
  );
  assert.deepEqual(calls, ['diagnose', 'host_connectivity']);
});

test('backend execution results preserve closed read and proposal routing', () => {
  const cases: Array<[AgentExecuteReadOnlyIntentResult, unknown]> = [
    [
      { status: 'diagnosed', snapshot: {} as never },
      { status: 'resolved', intent: { intent: 'diagnose' } },
    ],
    [
      { status: 'host_connectivity', connectivity: {} as never },
      { status: 'resolved', intent: { intent: 'host_connectivity' } },
    ],
    [
      {
        status: 'proposal_required',
        intent: { intent: 'set_system_proxy_enabled', enabled: true },
      },
      {
        status: 'resolved',
        intent: { intent: 'set_system_proxy_enabled', enabled: true },
      },
    ],
    [
      { status: 'unsupported', reason: 'no_matching_intent' },
      { status: 'unsupported', reason: 'no_matching_intent' },
    ],
  ];

  for (const [result, resolution] of cases) {
    assert.deepEqual(resolveExecutedReadOnlyIntent(result), resolution);
  }
});

test('write intents never enter automatic execution', async () => {
  const calls: string[] = [];
  const executed = await executeReadOnlyAgentIntent(
    { intent: 'set_system_proxy_enabled', enabled: true },
    {
      diagnose: () => calls.push('diagnose'),
      hostConnectivity: () => calls.push('host_connectivity'),
    },
  );

  assert.equal(executed, false);
  assert.deepEqual(calls, []);
});

test('closed write intents retain their explicit proposal actions', () => {
  const cases: Array<[AgentIntent, unknown]> = [
    [
      { intent: 'set_tun_enabled', enabled: true },
      { action: 'set_tun_enabled', enabled: true },
    ],
    [
      { intent: 'set_routing_mode', mode: 'rule' },
      { action: 'set_routing_mode', mode: 'rule' },
    ],
    [
      { intent: 'control_service', operation: 'restart' },
      { action: 'restart_service' },
    ],
    [
      { intent: 'repair_system_proxy_endpoint' },
      { action: 'repair_system_proxy_endpoint' },
    ],
  ];

  for (const [intent, action] of cases) {
    assert.deepEqual(routeAgentIntent(intent), { kind: 'proposal', action });
  }
});
