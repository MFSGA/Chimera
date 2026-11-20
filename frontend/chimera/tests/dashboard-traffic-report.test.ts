import assert from 'node:assert/strict';
import test from 'node:test';
import type { TrafficReport, Usage, UsageGroup } from '@chimera/interface';
import {
  addTrafficUsage,
  createTrafficReportRequest,
  sliceTrafficRanking,
  trafficSharePercent,
  uniqueTrafficReportRequests,
  usageTotalBytes,
  zeroTrafficUsage,
} from '../src/components/widgets/widget-traffic-report-model.ts';

const usage = (
  upload: number,
  download: number,
  connections: number,
): Usage => ({ bytes: { upload, download }, connections });
const group = (key: string, upload: number, download: number): UsageGroup => ({
  key,
  usage: usage(upload, download, 1),
  current_rate: null,
});

const report = (): TrafficReport => ({
  total: usage(210, 210, 6),
  current_rate: null,
  topology: null,
  rankings: [
    {
      dimension: 'origin',
      distinct: 6,
      groups: [
        group('unknown', 50, 50),
        group('/apps/second', 40, 40),
        group('third', 30, 30),
        group('fourth', 20, 20),
        group('fifth', 10, 10),
      ],
      other: usage(60, 60, 1),
    },
  ],
});

test('ref traffic request uses all dimensions and optional profile attribution', () => {
  assert.deepEqual(createTrafficReportRequest('last_hour', null), {
    metric: 'bytes',
    query: { range: 'last_hour', scope: 'all', filters: [] },
    rankings: ['origin', 'exit', 'target', 'rule'],
    ranking_limit: 20,
    topology: null,
  });
  assert.deepEqual(
    createTrafficReportRequest('last7_days', 'profile-uid').query.filters,
    [{ dimension: 'profile', value: 'profile-uid' }],
  );
});

test('identical widget requests reuse one report while preserving filter differences', () => {
  const requests = uniqueTrafficReportRequests([
    { range: 'last24_hours', profileUid: null },
    { range: 'last24_hours', profileUid: null },
    { range: 'last24_hours', profileUid: 'profile-uid' },
    { range: 'last7_days', profileUid: null },
  ]);
  assert.equal(requests.length, 3);
  assert.deepEqual(
    requests.map(({ query }) => [query.range, query.filters]),
    [
      ['last24_hours', []],
      ['last24_hours', [{ dimension: 'profile', value: 'profile-uid' }]],
      ['last7_days', []],
    ],
  );
});

test('top-three ranking folds hidden groups and server tail without lost bytes', () => {
  const sliced = sliceTrafficRanking(report(), 'origin', 3)!;
  assert.deepEqual(
    sliced.groups.map(({ key }) => key),
    ['unknown', '/apps/second', 'third'],
  );
  assert.deepEqual(sliced.other, usage(90, 90, 3));
  assert.equal(sliced.otherCount, 3);
  assert.equal(sliced.distinct, 6);
  assert.equal(sliced.totalBytes, 420);
  assert.equal(
    sliced.groups.reduce((sum, item) => sum + usageTotalBytes(item.usage), 0) +
      usageTotalBytes(sliced.other),
    sliced.totalBytes,
  );
  assert.deepEqual(
    [...sliced.groups.map(({ usage }) => usage), sliced.other].reduce(
      addTrafficUsage,
      zeroTrafficUsage(),
    ),
    report().total,
  );
});

test('top-five includes each visible row and only the original other tail', () => {
  const sliced = sliceTrafficRanking(report(), 'origin', 5)!;
  assert.deepEqual(
    sliced.groups.map(({ key }) => key),
    ['unknown', '/apps/second', 'third', 'fourth', 'fifth'],
  );
  assert.deepEqual(sliced.other, usage(60, 60, 1));
  assert.equal(sliced.otherCount, 1);
  assert.deepEqual(
    [...sliced.groups.map(({ usage }) => usage), sliced.other].reduce(
      addTrafficUsage,
      zeroTrafficUsage(),
    ),
    report().total,
  );
});

test('expanded ranking keeps the extended top-N limit', () => {
  const expanded = report();
  expanded.rankings[0]!.groups.push(
    group('sixth', 5, 5),
    group('seventh', 4, 4),
  );
  expanded.rankings[0]!.distinct = 8;
  const sliced = sliceTrafficRanking(expanded, 'origin', 7)!;
  assert.deepEqual(
    sliced.groups.map(({ key }) => key),
    ['unknown', '/apps/second', 'third', 'fourth', 'fifth', 'sixth', 'seventh'],
  );
  assert.equal(sliced.otherCount, 1);
});

test('unknown ranking and zero traffic are not misrepresented as percentages', () => {
  assert.equal(sliceTrafficRanking(report(), 'target', 3), null);
  assert.equal(trafficSharePercent(usage(0, 0, 0), 0), null);
  assert.equal(trafficSharePercent(usage(1, 2, 4), 6), 50);
});
