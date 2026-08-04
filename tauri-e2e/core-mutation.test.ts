import assert from 'node:assert/strict';
import test from 'node:test';
import {
  restartCoreSidecar,
  type RestartSidecarCommand,
} from '../frontend/interface/src/ipc/core-mutation.ts';

test('successful core restart resolves normally', async () => {
  let calls = 0;
  const commands: RestartSidecarCommand = {
    restartSidecar: async () => {
      calls += 1;
      return { status: 'ok', data: null };
    },
  };

  await restartCoreSidecar(commands);

  assert.equal(calls, 1);
});

test('failed core restart propagates the generated command error', async () => {
  const commands: RestartSidecarCommand = {
    restartSidecar: async () => ({
      status: 'error',
      error: 'restart rejected',
    }),
  };

  await assert.rejects(
    () => restartCoreSidecar(commands),
    (error) => error === 'restart rejected',
  );
});
