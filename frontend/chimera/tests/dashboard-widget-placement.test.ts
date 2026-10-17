import assert from 'node:assert/strict';
import test from 'node:test';
import type { WidgetId } from '../src/pages/(main)/main/dashboard/_modules/consts';
import { placeWidget } from '../src/pages/(main)/main/dashboard/_modules/widget-placement';

/*
 * Contract: direct additions choose an available cell; external drops honor
 * the requested cell and reject collisions; a full direct-add layout appends
 * below its current extent. This is a pure placement unit test, not UI/drag E2E.
 */
const minimum = { minW: 2, minH: 2 };
const memory = 'memory' as WidgetId;
const trafficDown = 'traffic-down' as WidgetId;
const occupied = [{ id: 'existing', type: memory, x: 0, y: 0, w: 2, h: 2 }];

test('places a library add in the first free cell', () => {
  assert.deepEqual(
    placeWidget({
      id: 'new-widget',
      type: trafficDown,
      items: occupied,
      cols: 4,
      rows: 4,
      minimum,
    }),
    {
      id: 'new-widget',
      type: trafficDown,
      x: 2,
      y: 0,
      w: 2,
      h: 2,
    },
  );
});

test('uses the drop cell and rejects a collision there', () => {
  assert.equal(
    placeWidget({
      id: 'new-widget',
      type: trafficDown,
      items: occupied,
      cols: 4,
      rows: 4,
      minimum,
      position: { x: 1, y: 1 },
    }),
    null,
  );

  assert.deepEqual(
    placeWidget({
      id: 'new-widget',
      type: trafficDown,
      items: occupied,
      cols: 4,
      rows: 4,
      minimum,
      position: { x: 2, y: 2 },
    }),
    {
      id: 'new-widget',
      type: trafficDown,
      x: 2,
      y: 2,
      w: 2,
      h: 2,
    },
  );
});

test('appends a direct add below the current layout when full', () => {
  const fullLayout = [
    { id: 'top-left', type: memory, x: 0, y: 0, w: 2, h: 2 },
    { id: 'top-right', type: memory, x: 2, y: 0, w: 2, h: 2 },
    { id: 'bottom-left', type: memory, x: 0, y: 2, w: 2, h: 2 },
    { id: 'bottom-right', type: memory, x: 2, y: 2, w: 2, h: 2 },
  ];

  assert.deepEqual(
    placeWidget({
      id: 'new-widget',
      type: trafficDown,
      items: fullLayout,
      cols: 4,
      rows: 4,
      minimum,
    }),
    {
      id: 'new-widget',
      type: trafficDown,
      x: 0,
      y: 4,
      w: 2,
      h: 2,
    },
  );
});
