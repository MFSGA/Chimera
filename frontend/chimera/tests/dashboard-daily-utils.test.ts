import assert from 'node:assert/strict';
import test from 'node:test';
import {
  calculateSubscriptionQuota,
  subscriptionExpiryTimestamp,
} from '../src/components/widgets/dashboard-daily-utils';

test('calculates remaining quota and percentage from both traffic directions', () => {
  assert.deepEqual(
    calculateSubscriptionQuota({ upload: 25, download: 75, total: 200 }),
    {
      upload: 25,
      download: 75,
      used: 100,
      total: 200,
      remaining: 100,
      usedPercent: 50,
      remainingPercent: 50,
      overage: 0,
    },
  );
});

test('clamps the percent and remaining bytes on overage without hiding it', () => {
  assert.deepEqual(
    calculateSubscriptionQuota({ upload: 101, download: 99, total: 100 }),
    {
      upload: 101,
      download: 99,
      used: 200,
      total: 100,
      remaining: 0,
      usedPercent: 100,
      remainingPercent: 0,
      overage: 100,
    },
  );
});

test('unknown or invalid quota is not reported as zero remaining', () => {
  for (const input of [
    undefined,
    { upload: 5, total: 20 },
    { upload: 5, download: 5 },
    { upload: 5, download: 5, total: 0 },
    { upload: Number.NaN, download: 1, total: 2 },
    { upload: -1, download: 1, total: 2 },
    { upload: 1, download: Number.POSITIVE_INFINITY, total: 2 },
    { upload: 1, download: 1, total: -2 },
  ]) {
    assert.equal(calculateSubscriptionQuota(input), null);
  }
});

test('subscription expiration converts seconds to milliseconds, rejecting invalid dates', () => {
  assert.equal(subscriptionExpiryTimestamp(1_800_000_000), 1_800_000_000_000);
  for (const input of [
    undefined,
    0,
    -1,
    Number.NaN,
    Number.POSITIVE_INFINITY,
    Number.MAX_VALUE,
  ]) {
    assert.equal(subscriptionExpiryTimestamp(input), null);
  }
});
