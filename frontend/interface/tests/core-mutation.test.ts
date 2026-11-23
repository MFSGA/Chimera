import assert from 'node:assert/strict';
import test from 'node:test';
import {
  restartCoreSidecar,
  type RestartSidecarCommand,
} from '../src/ipc/core-mutation.js';

test('ref Core rebuild reports success only after a successful Tauri result', async () => {
  const calls: string[] = [];
  const commands: RestartSidecarCommand = {
    restartSidecar: async () => {
      calls.push('restart');
      return { status: 'ok', data: null };
    },
  };
  await restartCoreSidecar(commands, () => calls.push('refresh'));
  assert.deepEqual(calls, ['restart', 'refresh']);
});

test('backend Core lifecycle errors are thrown, never treated as successful restarts', async () => {
  const message = 'CoreActor: service owner handoff is still in progress';
  const commands: RestartSidecarCommand = {
    restartSidecar: async () => ({ status: 'error', error: message }),
  };
  const phases: string[] = [];
  await assert.rejects(
    restartCoreSidecar(commands, () => phases.push('refresh')),
    (error: unknown) => {
      assert.equal(error, message);
      return true;
    },
  );
  assert.deepEqual(
    phases,
    ['refresh'],
    'even an unconfirmed mutation refreshes ref status',
  );
});

test('lost Tauri command replies are not swallowed by the restart adapter', async () => {
  const unavailable = new Error('Tauri runtime disconnected');
  const commands: RestartSidecarCommand = {
    restartSidecar: async () => {
      throw unavailable;
    },
  };
  const refreshes: string[] = [];
  await assert.rejects(
    restartCoreSidecar(commands, () =>
      refreshes.push('ref status invalidated'),
    ),
    (error: unknown) => {
      assert.equal(error, unavailable);
      return true;
    },
  );
  assert.deepEqual(refreshes, ['ref status invalidated']);
});
