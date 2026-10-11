import assert from 'node:assert/strict';
import test from 'node:test';
import type { AgentIntentResolution } from '@chimera/interface';
import { resolveAgentIntentForDisplay } from './intent-resolution';

const diagnosis: AgentIntentResolution = {
  status: 'resolved',
  intent: { intent: 'diagnose' },
};

test('diagnosis success is visible only after a successful snapshot refresh', async () => {
  let completeRefresh!: (result: { isSuccess: boolean }) => void;
  const refresh = new Promise<{ isSuccess: boolean }>((resolve) => {
    completeRefresh = resolve;
  });
  let completed = false;
  const pending = resolveAgentIntentForDisplay(
    'diagnose',
    async () => diagnosis,
    () => refresh,
  ).then((result) => {
    completed = true;
    return result;
  });

  await Promise.resolve();
  assert.equal(completed, false, 'pending refresh must not report success');

  completeRefresh({ isSuccess: true });
  assert.deepEqual(await pending, diagnosis);
});

test('a failed snapshot refresh does not report diagnosis success', async () => {
  assert.equal(
    await resolveAgentIntentForDisplay(
      'diagnose',
      async () => diagnosis,
      async () => ({ isSuccess: false }),
    ),
    null,
  );
});

test('non-diagnostic intents do not refresh the snapshot', async () => {
  let refreshes = 0;
  const refresh = async () => {
    refreshes++;
    return { isSuccess: true };
  };
  const unsupported: AgentIntentResolution = {
    status: 'unsupported',
    reason: 'no_matching_intent',
  };
  const proposal: AgentIntentResolution = {
    status: 'resolved',
    intent: { intent: 'set_tun_enabled', enabled: true },
  };

  assert.deepEqual(
    await resolveAgentIntentForDisplay(
      'unknown',
      async () => unsupported,
      refresh,
    ),
    unsupported,
  );
  assert.deepEqual(
    await resolveAgentIntentForDisplay(
      'enable tun',
      async () => proposal,
      refresh,
    ),
    proposal,
  );
  assert.equal(refreshes, 0);
});

test('snapshot refresh exceptions propagate as errors', async () => {
  await assert.rejects(
    resolveAgentIntentForDisplay(
      'diagnose',
      async () => diagnosis,
      async () => {
        throw new Error('diagnosis unavailable');
      },
    ),
    /diagnosis unavailable/,
  );
});
