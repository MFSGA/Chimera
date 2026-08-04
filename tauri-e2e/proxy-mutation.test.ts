import assert from 'node:assert/strict';
import test from 'node:test';
import {
  selectProxyAndRefresh,
  type SelectProxyCommand,
} from '../frontend/interface/src/ipc/proxy-mutation.ts';

test('successful proxy selection refreshes the proxy query once', async () => {
  const calls: string[] = [];
  const commands: SelectProxyCommand = {
    selectProxy: async (group, name) => {
      calls.push(`select:${group}:${name}`);
      return { status: 'ok', data: null };
    },
  };

  await selectProxyAndRefresh(commands, 'GLOBAL', 'Auto', async () => {
    calls.push('refetch');
  });

  assert.deepEqual(calls, ['select:GLOBAL:Auto', 'refetch']);
});

test('failed proxy selection propagates the error and skips refresh', async () => {
  let refetches = 0;
  const commands: SelectProxyCommand = {
    selectProxy: async () => ({
      status: 'error',
      error: 'selection rejected',
    }),
  };

  await assert.rejects(
    () =>
      selectProxyAndRefresh(commands, 'GLOBAL', 'Auto', async () => {
        refetches += 1;
      }),
    (error) => error === 'selection rejected',
  );
  assert.equal(refetches, 0);
});
