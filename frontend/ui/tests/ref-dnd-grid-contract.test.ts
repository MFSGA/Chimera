/**
 * Pure geometry regressions based on the pinned ref DndGrid algorithms.
 * This does not simulate pointer/keyboard/desktop drag interactions.
 */
import assert from 'node:assert/strict';
import test from 'node:test';
import type { DndGridItemType } from '../src/dnd-grid/types';
import { calculateResize, hasOverlap, isOverlap } from '../src/dnd-grid/utils';

const moving: DndGridItemType = {
  id: 'moving',
  x: 2,
  y: 2,
  w: 2,
  h: 2,
};

test('adjacent grid cells do not overlap, intersecting rectangles do', () => {
  assert.equal(
    isOverlap(moving, { id: 'right', x: 4, y: 2, w: 1, h: 2 }),
    false,
  );
  assert.equal(
    isOverlap(moving, { id: 'bottom', x: 2, y: 4, w: 2, h: 1 }),
    false,
  );
  assert.equal(
    isOverlap(moving, { id: 'diagonal', x: 4, y: 4, w: 1, h: 1 }),
    false,
  );
  assert.equal(
    isOverlap(moving, { id: 'intersect', x: 3, y: 3, w: 2, h: 2 }),
    true,
  );
});

test('candidate overlap ignores the moving item but never another widget', () => {
  const widgets: DndGridItemType[] = [
    moving,
    { id: 'another', x: 4, y: 2, w: 1, h: 2 },
  ];
  assert.equal(hasOverlap(widgets, 'moving', moving), false);
  assert.equal(hasOverlap(widgets, 'moving', { ...moving, x: 3 }), true);
  assert.equal(hasOverlap(widgets, 'moving', { ...moving, y: 4 }), false);
});

test('resize snaps to cells, preserves opposite edge, and respects bounds', () => {
  const resize = (
    handle: Parameters<typeof calculateResize>[1],
    dx: number,
    dy: number,
    constraints: Parameters<typeof calculateResize>[9] = {},
  ) => calculateResize(moving, handle, dx, dy, 100, 100, 0, 6, 6, constraints);

  assert.deepEqual(resize('right', 100, 0), { ...moving, w: 3 });
  assert.deepEqual(resize('left', -100, 0), { ...moving, x: 1, w: 3 });
  assert.deepEqual(resize('top', 0, -100), { ...moving, y: 1, h: 3 });
  assert.deepEqual(resize('bottom-right', 100, 100), { ...moving, w: 3, h: 3 });
  assert.deepEqual(resize('right', 5000, 0), { ...moving, w: 4 });
  assert.deepEqual(resize('left', 1000, 0), { ...moving, x: 3, w: 1 });
  assert.deepEqual(resize('right', 100, 0, { maxW: 2 }), moving);
  assert.deepEqual(resize('bottom', 0, -100, { minH: 2 }), moving);
});
