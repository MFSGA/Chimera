import assert from 'node:assert/strict';
import test from 'node:test';
import { commands } from '../src/ipc/bindings.js';
import { mutations } from '../src/ipc/query-bindings.js';
import {
  invokeMutation,
  MutationUnconfirmedError,
} from '../src/ipc/query-options.js';
import { unwrapResult } from '../src/utils/index.js';

test('native group test invokes one instance-bound IPC mutation instead of per-node IPC', async () => {
  const original = commands.testNativeProxyGroupDelay;
  const originalGroup = commands.clashApiGetGroupDelay;
  const originalNode = commands.clashApiGetProxyDelay;
  const originalSnapshot = commands.getNativeProxies;
  const calls: string[] = [];
  commands.testNativeProxyGroupDelay = async (name) => {
    calls.push(name);
    return {
      status: 'ok',
      data: { 'node-a': 37, 'node-b': 0 },
    };
  };
  commands.clashApiGetGroupDelay = async () => {
    throw new Error('group policy must execute on the core instance');
  };
  commands.clashApiGetProxyDelay = async () => {
    throw new Error('node requests must not acquire new frontend IPC clients');
  };
  commands.getNativeProxies = async () => {
    throw new Error('controller group refresh belongs to core operation');
  };
  try {
    const action = mutations.testNativeProxyGroupDelay;
    assert.deepEqual(action.mutationKey, ['testNativeProxyGroupDelay']);
    assert.deepEqual(unwrapResult(await invokeMutation(action, ['Auto'])), {
      'node-a': 37,
      'node-b': 0,
    });
    assert.deepEqual(calls, ['Auto']);
  } finally {
    commands.testNativeProxyGroupDelay = original;
    commands.clashApiGetGroupDelay = originalGroup;
    commands.clashApiGetProxyDelay = originalNode;
    commands.getNativeProxies = originalSnapshot;
  }
});

test('missing native group test reply is unconfirmed and never dispatched again', async () => {
  const original = commands.testNativeProxyGroupDelay;
  let calls = 0;
  commands.testNativeProxyGroupDelay = async () => {
    calls += 1;
    throw new Error('lost IPC response');
  };
  try {
    const action = mutations.testNativeProxyGroupDelay;
    await assert.rejects(
      () => invokeMutation(action, ['Auto']),
      (error: unknown) => error instanceof MutationUnconfirmedError,
    );
    assert.equal(calls, 1, 'one submitted group test is never replayed');
  } finally {
    commands.testNativeProxyGroupDelay = original;
  }
});
