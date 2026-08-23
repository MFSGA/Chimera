import assert from 'node:assert/strict';
import test from 'node:test';
import {
  runProfileAction,
  runProfileOrderAction,
} from '../frontend/chimera/src/pages/(main)/main/profiles/$type/_modules/profile-action.js';

test('successful profile action returns its value without reporting an error', async () => {
  let executions = 0;
  let reports = 0;

  const result = await runProfileAction(
    async () => {
      executions += 1;
      return 'saved';
    },
    () => {
      reports += 1;
    },
  );

  assert.equal(result, 'saved');
  assert.equal(executions, 1);
  assert.equal(reports, 0);
});

test('failed profile action reports the original error once and returns undefined', async () => {
  const failure = new Error('profile order was rolled back');
  const reported: unknown[] = [];

  const result = await runProfileAction(
    async () => {
      throw failure;
    },
    (error) => {
      reported.push(error);
    },
  );

  assert.equal(result, undefined);
  assert.deepEqual(reported, [failure]);
});

test('profile action does not hide an error thrown by the reporter', async () => {
  const reportingFailure = new Error('notice provider unavailable');

  await assert.rejects(
    runProfileAction(
      async () => {
        throw new Error('profile action failed');
      },
      () => {
        throw reportingFailure;
      },
    ),
    reportingFailure,
  );
});

test('profile order action submits the merged full order once', async () => {
  const submissions: string[][] = [];
  const errors: unknown[] = [];

  const result = await runProfileOrderAction(
    ['profile-a', 'script-a', 'profile-b'],
    ['profile-a', 'profile-b'],
    ['profile-b', 'profile-a'],
    async (fullOrder) => {
      submissions.push(fullOrder);
      return 'saved';
    },
    (error) => errors.push(error),
  );

  assert.equal(result, 'saved');
  assert.deepEqual(submissions, [['profile-b', 'script-a', 'profile-a']]);
  assert.deepEqual(errors, []);
});

test('invalid profile order is reported without submitting a mutation', async () => {
  let submissions = 0;
  const errors: unknown[] = [];

  const result = await runProfileOrderAction(
    ['profile-a', 'script-a'],
    ['profile-a', 'missing-profile'],
    ['missing-profile', 'profile-a'],
    async () => {
      submissions += 1;
      return 'unexpected';
    },
    (error) => errors.push(error),
  );

  assert.equal(result, undefined);
  assert.equal(submissions, 0);
  assert.equal(errors.length, 1);
  assert.match(String(errors[0]), /missing from the full order/);
});

test('profile order submission failure is reported with the original error', async () => {
  const failure = new Error('profile order persistence failed');
  const errors: unknown[] = [];

  const result = await runProfileOrderAction(
    ['profile-a', 'profile-b'],
    ['profile-a', 'profile-b'],
    ['profile-b', 'profile-a'],
    async () => {
      throw failure;
    },
    (error) => errors.push(error),
  );

  assert.equal(result, undefined);
  assert.deepEqual(errors, [failure]);
});
