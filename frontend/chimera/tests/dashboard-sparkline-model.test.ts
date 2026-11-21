import assert from 'node:assert/strict';
import test from 'node:test';
import {
  connectionCount,
  connectionSamples,
  padSparklineData,
  trafficSamples,
} from '../src/components/widgets/widget-sparkline-model.ts';

test('traffic series adapts Chimera up/down snapshot fields to ref chart order', () => {
  const traffic = [
    { up: 100, down: 200 },
    { up: 150, down: 250 },
    { up: 175, down: 300 },
  ];
  assert.deepEqual(trafficSamples(traffic, 'down', 4), [0, 200, 250, 300]);
  assert.deepEqual(trafficSamples(traffic, 'up', 2), [150, 175]);
  assert.deepEqual(traffic, [
    { up: 100, down: 200 },
    { up: 150, down: 250 },
    { up: 175, down: 300 },
  ]);
});

test('empty or missing live traffic still draws a stable zero baseline', () => {
  assert.deepEqual(trafficSamples(undefined, 'down', 3), [0, 0, 0]);
  assert.deepEqual(trafficSamples([], 'up', 0), []);
});

test('Chimera connection snapshots use connections array length instead of ref connectionCount', () => {
  const snapshot = (n: number) => ({
    downloadTotal: 0,
    uploadTotal: 0,
    downloadSpeed: 0,
    uploadSpeed: 0,
    connections: Array.from({ length: n }, (_, index) => ({
      id: String(index),
    })),
  });
  assert.equal(connectionCount(undefined), 0);
  assert.equal(connectionCount(snapshot(4) as never), 4);
  assert.deepEqual(
    connectionSamples([snapshot(3), snapshot(5)] as never, 4),
    [0, 0, 3, 5],
  );
  assert.deepEqual(
    connectionSamples([snapshot(1), snapshot(2), snapshot(3)] as never, 2),
    [2, 3],
  );
});

test('history padding replaces absent samples with zero and trims to requested width', () => {
  assert.deepEqual(
    padSparklineData([undefined, 2, undefined], 4),
    [0, 0, 2, 0],
  );
  assert.deepEqual(padSparklineData([1, 2, 3, 4], 2), [3, 4]);
  assert.deepEqual(padSparklineData([], 0), []);
});
