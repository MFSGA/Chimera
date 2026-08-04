import assert from 'node:assert/strict';
import test from 'node:test';
import { getProfileSubscriptionUsage } from '../frontend/interface/src/utils/profile-subscription.ts';

test('profile subscription usage calculates a bounded percentage', () => {
  assert.deepEqual(getProfileSubscriptionUsage(25, 25, 100), {
    progress: 50,
    total: 100,
    used: 50,
  });
  assert.deepEqual(getProfileSubscriptionUsage(80, 40, 100), {
    progress: 100,
    total: 100,
    used: 120,
  });
});

test('profile subscription usage accepts zero and missing counters', () => {
  assert.deepEqual(getProfileSubscriptionUsage(undefined, null, 0), {
    progress: 0,
    total: 0,
    used: 0,
  });
});

test('profile subscription usage ignores malformed historical counters', () => {
  assert.deepEqual(
    getProfileSubscriptionUsage(
      Number.NaN,
      Number.POSITIVE_INFINITY,
      Number.MAX_SAFE_INTEGER + 1,
    ),
    { progress: 0, total: 0, used: 0 },
  );
  assert.deepEqual(getProfileSubscriptionUsage(-1, 1.5, 100), {
    progress: 0,
    total: 100,
    used: 0,
  });
});

test('profile subscription usage rejects unsafe upload and download sums', () => {
  assert.deepEqual(
    getProfileSubscriptionUsage(
      Number.MAX_SAFE_INTEGER,
      Number.MAX_SAFE_INTEGER,
      Number.MAX_SAFE_INTEGER,
    ),
    { progress: 0, total: Number.MAX_SAFE_INTEGER, used: 0 },
  );
});
