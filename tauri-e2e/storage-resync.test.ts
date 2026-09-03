import assert from 'node:assert/strict';
import test from 'node:test';
import {
  createStorageResyncCoordinator,
  reconcileStorageSnapshot,
} from '../frontend/chimera/src/services/storage-resync.js';

test('storage snapshot refreshes subscribed keys and clears missing values', () => {
  const observed: Array<[string, unknown | null]> = [];

  reconcileStorageSnapshot(
    ['web:route', 'web:theme', 'web:removed'],
    {
      'web:route': '/settings',
      'web:theme': 'dark',
      'internal:history': 'must-not-leak',
    },
    (key, value) => observed.push([key, value]),
  );

  assert.deepEqual(observed, [
    ['web:route', '/settings'],
    ['web:theme', 'dark'],
    ['web:removed', null],
  ]);
});

test('storage resync coalesces repeated signals into one queued refresh', async () => {
  const resolvers: Array<(snapshot: Record<string, unknown>) => void> = [];
  const applied: Array<Record<string, unknown>> = [];
  let activeLoads = 0;
  let maxActiveLoads = 0;
  const coordinator = createStorageResyncCoordinator(
    () => {
      activeLoads += 1;
      maxActiveLoads = Math.max(maxActiveLoads, activeLoads);
      return new Promise((resolve) => {
        resolvers.push((snapshot) => {
          activeLoads -= 1;
          resolve(snapshot);
        });
      });
    },
    (snapshot) => applied.push(snapshot),
    (error) => assert.fail(error instanceof Error ? error : String(error)),
  );

  const first = coordinator.resync();
  const second = coordinator.resync();
  const third = coordinator.resync();

  assert.equal(resolvers.length, 1);
  assert.strictEqual(second, first);
  assert.strictEqual(third, first);

  resolvers[0]({ 'web:route': '/first' });
  await new Promise<void>((resolve) => setImmediate(resolve));
  assert.equal(resolvers.length, 2);

  resolvers[1]({ 'web:route': '/latest' });
  await first;

  assert.equal(maxActiveLoads, 1);
  assert.deepEqual(applied, [
    { 'web:route': '/first' },
    { 'web:route': '/latest' },
  ]);
});

test('storage resync recovers after a queued refresh fails', async () => {
  const outcomes: Array<
    | { type: 'resolve'; snapshot: Record<string, unknown> }
    | { type: 'reject'; error: Error }
  > = [
    { type: 'resolve', snapshot: { 'web:route': '/initial' } },
    { type: 'reject', error: new Error('snapshot unavailable') },
    { type: 'resolve', snapshot: { 'web:route': '/recovered' } },
  ];
  const applied: Array<Record<string, unknown>> = [];
  const errors: unknown[] = [];
  let loadCount = 0;
  const coordinator = createStorageResyncCoordinator(
    async () => {
      const outcome = outcomes[loadCount++];
      if (outcome.type === 'reject') throw outcome.error;
      return outcome.snapshot;
    },
    (snapshot) => applied.push(snapshot),
    (error) => errors.push(error),
    { maxRetries: 0 },
  );

  const first = coordinator.resync();
  coordinator.resync();
  await first;

  assert.equal(loadCount, 2);
  assert.deepEqual(applied, [{ 'web:route': '/initial' }]);
  assert.equal(errors.length, 1);
  assert.match(String(errors[0]), /snapshot unavailable/);

  await coordinator.resync();

  assert.equal(loadCount, 3);
  assert.deepEqual(applied, [
    { 'web:route': '/initial' },
    { 'web:route': '/recovered' },
  ]);
  assert.equal(errors.length, 1);
});

test('storage resync retries with backoff and a new signal wakes it early', async () => {
  const sleepResolvers: Array<() => void> = [];
  const applied: Array<Record<string, unknown>> = [];
  const errors: unknown[] = [];
  let loadCount = 0;
  let sleepCount = 0;
  const coordinator = createStorageResyncCoordinator(
    async () => {
      loadCount += 1;
      if (loadCount === 1) throw new Error('temporary failure');
      return { 'web:route': '/recovered' };
    },
    (snapshot) => applied.push(snapshot),
    (error) => errors.push(error),
    {
      maxRetries: 1,
      retryDelayMs: 10_000,
      sleep: async () => {
        sleepCount += 1;
        await new Promise<void>((resolve) => sleepResolvers.push(resolve));
      },
    },
  );

  const pending = coordinator.resync();
  await Promise.resolve();
  await Promise.resolve();

  assert.equal(loadCount, 1);
  assert.equal(sleepCount, 1);
  assert.deepEqual(applied, []);

  coordinator.resync();
  await pending;

  assert.equal(loadCount, 3);
  assert.equal(sleepCount, 1);
  assert.deepEqual(applied, [
    { 'web:route': '/recovered' },
    { 'web:route': '/recovered' },
  ]);
  assert.deepEqual(errors, []);
});

test('disposed storage resync ignores late snapshots and errors', async () => {
  let resolveSnapshot!: (snapshot: Record<string, unknown>) => void;
  const applied: Array<Record<string, unknown>> = [];
  const errors: unknown[] = [];
  const coordinator = createStorageResyncCoordinator(
    () =>
      new Promise((resolve) => {
        resolveSnapshot = resolve;
      }),
    (snapshot) => applied.push(snapshot),
    (error) => errors.push(error),
  );

  const pending = coordinator.resync();
  coordinator.dispose();
  resolveSnapshot({ 'web:route': '/late' });
  await pending;

  assert.deepEqual(applied, []);
  assert.deepEqual(errors, []);
});

test('storage snapshot safely handles keys inherited from the prototype', () => {
  const observed: Array<[string, unknown | null]> = [];
  const snapshot = Object.create({ 'web:route': '/stale' }) as Record<
    string,
    unknown
  >;

  reconcileStorageSnapshot(['web:route'], snapshot, (key, value) =>
    observed.push([key, value]),
  );

  assert.deepEqual(observed, [['web:route', null]]);
});
