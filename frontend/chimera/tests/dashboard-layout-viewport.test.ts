import assert from 'node:assert/strict';
import test from 'node:test';
import { createDashboardViewportCoordinator } from '../src/pages/(main)/main/dashboard/_modules/layout-viewport.ts';

const size = (cols: number, rows: number) => ({ cols, rows });

test('initial viewport applies immediately; duplicate resize is ignored', () => {
  const applied: ReturnType<typeof size>[] = [];
  const viewport = createDashboardViewportCoordinator((next) =>
    applied.push(next),
  );
  viewport.onSizeChange(size(8, 6));
  viewport.onSizeChange(size(8, 6));
  assert.deepEqual(applied, [size(8, 6)]);
  assert.equal(viewport.hasPendingSize(), false);
});

test('resizing during drag defers layout changes and rejects an old-grid drop', () => {
  const applied: ReturnType<typeof size>[] = [];
  const viewport = createDashboardViewportCoordinator((next) =>
    applied.push(next),
  );
  viewport.onSizeChange(size(8, 6));
  viewport.onInteractionChange(true);
  viewport.onSizeChange(size(5, 6));
  viewport.onSizeChange(size(3, 5));
  assert.equal(viewport.isInteracting(), true);
  assert.equal(viewport.shouldDiscardCommit(), true);
  assert.deepEqual(applied, [size(8, 6)]);
  viewport.onInteractionChange(false);
  assert.deepEqual(applied, [size(8, 6), size(3, 5)]);
  assert.equal(viewport.hasPendingSize(), false);
});

test('unchanged dimensions still reapply after cancelled settle timer', () => {
  const applied: ReturnType<typeof size>[] = [];
  const viewport = createDashboardViewportCoordinator((next) =>
    applied.push(next),
  );
  viewport.onSizeChange(size(8, 6));
  viewport.onInteractionChange(true);
  assert.equal(viewport.shouldDiscardCommit(), false);
  viewport.onInteractionChange(false);
  assert.deepEqual(applied, [size(8, 6), size(8, 6)]);
});

test('a pending KV save defers viewport replay until confirmation', () => {
  const applied: ReturnType<typeof size>[] = [];
  const viewport = createDashboardViewportCoordinator((next) =>
    applied.push(next),
  );
  viewport.onSizeChange(size(8, 6));
  viewport.onInteractionChange(true);
  viewport.onSavingChange(true);
  viewport.onInteractionChange(false);
  viewport.onSizeChange(size(4, 6));
  assert.deepEqual(applied, [size(8, 6)]);
  assert.equal(viewport.hasPendingSize(), true);
  viewport.onSavingChange(false);
  assert.deepEqual(applied, [size(8, 6), size(4, 6)]);
  assert.equal(viewport.hasPendingSize(), false);
});

test('failed save also permits viewport recovery, and new gesture resets old invalid-drop flag', () => {
  const applied: ReturnType<typeof size>[] = [];
  const viewport = createDashboardViewportCoordinator((next) =>
    applied.push(next),
  );
  viewport.onSizeChange(size(8, 6));
  viewport.onInteractionChange(true);
  viewport.onSizeChange(size(6, 6));
  assert.equal(viewport.shouldDiscardCommit(), true);
  viewport.onSavingChange(true);
  viewport.onInteractionChange(false);
  viewport.onSavingChange(false); // failed but now settled
  assert.deepEqual(applied.at(-1), size(6, 6));
  viewport.onInteractionChange(true);
  assert.equal(viewport.shouldDiscardCommit(), false);
  viewport.onInteractionChange(false);
});

test('direct-add save postpones a pending viewport settle even without grid drag', () => {
  const applied: ReturnType<typeof size>[] = [];
  const viewport = createDashboardViewportCoordinator((next) =>
    applied.push(next),
  );
  viewport.onSizeChange(size(8, 6));
  viewport.onSavingChange(true);
  viewport.onSizeChange(size(8, 6));
  assert.deepEqual(applied, [size(8, 6)]);
  viewport.onSavingChange(false);
  assert.deepEqual(applied, [size(8, 6), size(8, 6)]);
});

test('restored viewport chooses the latest pending size and never applies stale intermediate values', () => {
  const applied: ReturnType<typeof size>[] = [];
  const viewport = createDashboardViewportCoordinator((next) =>
    applied.push(next),
  );
  viewport.onSizeChange(size(10, 8));
  viewport.onSavingChange(true);
  viewport.onSizeChange(size(2, 4));
  viewport.onSizeChange(size(10, 8));
  viewport.onSizeChange(size(7, 5));
  assert.deepEqual(applied, [size(10, 8)]);
  viewport.onSavingChange(false);
  assert.deepEqual(applied, [size(10, 8), size(7, 5)]);
});
