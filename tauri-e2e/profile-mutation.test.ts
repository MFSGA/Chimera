import assert from 'node:assert/strict';
import test from 'node:test';
import {
  ProfileChangeRolledBackError,
  runProfileRebuildMutation,
} from '../frontend/interface/src/ipc/profile-mutation.ts';

test('successful profile rebuild invalidates profiles once', async () => {
  let invalidations = 0;

  const outcome = await runProfileRebuildMutation(
    async () => ({ status: 'ok' }),
    async () => {
      invalidations += 1;
    },
  );

  assert.deepEqual(outcome, { status: 'ok' });
  assert.equal(invalidations, 1);
});

test('degraded profile rebuild rejects and skips profile invalidation', async () => {
  let invalidations = 0;

  await assert.rejects(
    runProfileRebuildMutation(
      async () => ({
        status: 'degraded',
        error:
          'new core configuration failed and the previous profile was restored',
      }),
      async () => {
        invalidations += 1;
      },
    ),
    (error: unknown) => {
      assert.ok(error instanceof ProfileChangeRolledBackError);
      assert.match(error.message, /previous profile was restored/);
      return true;
    },
  );

  assert.equal(invalidations, 0);
});

test('missing profile rebuild outcome rejects and skips invalidation', async () => {
  let invalidations = 0;

  await assert.rejects(
    runProfileRebuildMutation(
      async () => undefined,
      async () => {
        invalidations += 1;
      },
    ),
    /no rebuild outcome/,
  );

  assert.equal(invalidations, 0);
});

test('profile command rejection propagates and skips invalidation', async () => {
  const failure = new Error('profile IPC failed');
  let invalidations = 0;

  await assert.rejects(
    runProfileRebuildMutation(
      async () => {
        throw failure;
      },
      async () => {
        invalidations += 1;
      },
    ),
    failure,
  );

  assert.equal(invalidations, 0);
});

test('profile invalidation rejection propagates after a successful rebuild', async () => {
  const failure = new Error('profile cache refresh failed');
  let executions = 0;
  let invalidations = 0;

  await assert.rejects(
    runProfileRebuildMutation(
      async () => {
        executions += 1;
        return { status: 'ok' };
      },
      async () => {
        invalidations += 1;
        throw failure;
      },
    ),
    failure,
  );

  assert.equal(executions, 1);
  assert.equal(invalidations, 1);
});

test('degraded profile rebuild preserves an empty backend reason', async () => {
  await assert.rejects(
    runProfileRebuildMutation(
      async () => ({ status: 'degraded', error: '' }),
      async () => {
        throw new Error('invalidation must not run for degraded outcomes');
      },
    ),
    (error: unknown) => {
      assert.ok(error instanceof ProfileChangeRolledBackError);
      assert.equal(error.message, 'Profile change was rolled back: ');
      return true;
    },
  );
});
