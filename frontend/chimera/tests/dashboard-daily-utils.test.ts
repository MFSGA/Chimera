import assert from 'node:assert/strict';
import test from 'node:test';
import type { ProfileItem_Serialize, SyncRunSummary } from '@chimera/interface';
import {
  calculateSubscriptionQuota,
  getRemoteFileConfigs,
  isExpiryWarning,
  isQuotaWarning,
  latestFinishedRuns,
  resolveProfileTarget,
  subscriptionExpiryTimestamp,
} from '../src/components/widgets/dashboard-daily-utils';

const remoteConfig = (uid: string): ProfileItem_Serialize =>
  ({
    uid,
    name: uid,
    type: 'config',
    config: { type: 'file', source: { type: 'remote' } },
  }) as ProfileItem_Serialize;

const composition = (uid: string): ProfileItem_Serialize =>
  ({
    uid,
    name: uid,
    type: 'config',
    config: { type: 'composition' },
  }) as ProfileItem_Serialize;

const transform = (uid: string): ProfileItem_Serialize =>
  ({ uid, name: uid, type: 'transform' }) as ProfileItem_Serialize;

test('resolves a remote profile without silently substituting composed or missing targets', () => {
  const items = [
    remoteConfig('remote-a'),
    composition('combined'),
    transform('script'),
  ];
  assert.deepEqual(
    resolveProfileTarget(items, 'remote-a', { kind: 'current' }),
    {
      kind: 'selected',
      profile: items[0],
    },
  );
  assert.deepEqual(
    resolveProfileTarget(items, 'combined', { kind: 'current' }),
    {
      kind: 'needs_selection',
    },
  );
  assert.deepEqual(resolveProfileTarget(items, 'script', { kind: 'current' }), {
    kind: 'unsupported',
    profile: items[2],
  });
  assert.deepEqual(resolveProfileTarget(items, null, { kind: 'current' }), {
    kind: 'needs_selection',
  });
  assert.deepEqual(
    resolveProfileTarget(items, 'remote-a', {
      kind: 'fixed',
      profileUid: 'removed',
    }),
    { kind: 'missing' },
  );
  assert.deepEqual(
    resolveProfileTarget(items, 'combined', {
      kind: 'fixed',
      profileUid: 'remote-a',
    }),
    { kind: 'selected', profile: items[0] },
  );
  assert.deepEqual(
    getRemoteFileConfigs(items).map((item) => item.uid),
    ['remote-a'],
  );
  assert.deepEqual(getRemoteFileConfigs(items, ['removed']), []);
});

test('recent summary runs exclude active work and sort large admission sequences exactly', () => {
  const finished = {
    kind: 'finished' as const,
    outcome: 'succeeded' as const,
    journal_degraded: false,
  };
  const summary = (id: string, sequence: string): SyncRunSummary => ({
    id,
    admission_sequence: sequence,
    trigger: 'interval',
    scheduled_at: null,
    admitted_at: '2026-01-01T00:00:00Z',
    finished_at: '2026-01-01T00:00:01Z',
    state: finished,
  });
  const runs = [
    summary('old', '9007199254740992'),
    {
      ...summary('active', '9007199254740995'),
      state: { kind: 'running' as const },
    },
    summary('new', '9007199254740994'),
    summary('middle', '9007199254740993'),
  ];
  assert.deepEqual(
    latestFinishedRuns(runs, 2).map((run) => run.id),
    ['new', 'middle'],
  );
  assert.deepEqual(latestFinishedRuns(runs, 0), []);
});

test('quota and expiry warnings match ref boundaries and leave unknown data unknown', () => {
  const now = Date.parse('2026-11-20T00:00:00Z');
  const day = 24 * 60 * 60 * 1000;
  assert.equal(isExpiryWarning(null, 7, now), false);
  assert.equal(isExpiryWarning(now + 7 * day, 7, now), true);
  assert.equal(isExpiryWarning(now + 7 * day + 1, 7, now), false);
  assert.equal(isExpiryWarning(now - 1, 7, now), true);
  assert.equal(isQuotaWarning(null, 20), false);
  const quota = calculateSubscriptionQuota({
    upload: 40,
    download: 40,
    total: 100,
  });
  assert.equal(isQuotaWarning(quota, 20), true);
  assert.equal(isQuotaWarning(quota, 19), false);
});

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
