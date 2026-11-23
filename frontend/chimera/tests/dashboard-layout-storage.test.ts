import assert from 'node:assert/strict';
import test from 'node:test';
import type {
  DashboardItem,
  LayoutStorage,
} from '../src/components/widgets/consts.ts';
import { WidgetId } from '../src/components/widgets/widget-config.ts';
import { normalizeDashboardItems } from '../src/components/widgets/widget-layout.ts';
import { adaptLayout } from '../src/pages/(main)/main/dashboard/_modules/layout-adapt.ts';
import {
  effectiveLayoutSize,
  normalizeLayoutStorage,
  saveLayout,
  selectLayoutEntry,
} from '../src/pages/(main)/main/dashboard/_modules/layout-storage.ts';

const item = (
  id: WidgetId,
  x: number,
  y: number,
  w: number,
  h: number,
): DashboardItem => ({ id, type: id, x, y, w, h });
const defaults: LayoutStorage = {
  '4x5': [item(WidgetId.TrafficDown, 0, 0, 2, 2)],
};

test('ref v2 migration preserves different legacy entries with identical footprints', () => {
  const first = [item(WidgetId.TrafficDown, 0, 0, 2, 2)];
  const second = [item(WidgetId.TrafficUp, 0, 0, 2, 2)];
  const storage = normalizeLayoutStorage(
    { '4x5': first, '12x6': second },
    defaults,
  );
  assert.deepEqual(storage.entries, [
    { id: 'legacy:4x5', size: { cols: 2, rows: 2 }, items: first },
    { id: 'legacy:12x6', size: { cols: 2, rows: 2 }, items: second },
  ]);
  assert.equal(storage.preferredId, null);
});

test('effective size is the stored footprint, not unused viewport bounds', () => {
  const items = [item(WidgetId.TrafficDown, 1, 2, 3, 2)];
  assert.deepEqual(effectiveLayoutSize(items), { cols: 4, rows: 4 });
  assert.deepEqual(
    normalizeLayoutStorage({ '12x8': items }, defaults).entries[0]?.size,
    { cols: 4, rows: 4 },
  );
});

test('chooses largest fitting layout but keeps explicitly preferred edited entry', () => {
  const first = [item(WidgetId.TrafficDown, 0, 0, 2, 2)];
  const second = [item(WidgetId.TrafficUp, 0, 0, 4, 4)];
  const storage = normalizeLayoutStorage(
    { '2x2': first, '4x4': second },
    defaults,
  );
  assert.equal(
    selectLayoutEntry(storage, { cols: 8, rows: 8 })?.id,
    'legacy:4x4',
  );
  const preferred = saveLayout(storage, 'legacy:2x2', first);
  assert.equal(
    selectLayoutEntry(preferred, { cols: 8, rows: 8 })?.id,
    'legacy:2x2',
  );
  assert.equal(
    selectLayoutEntry(preferred, { cols: 1, rows: 1 })?.id,
    'legacy:2x2',
  );
});

test('saving a preferred layout retains a distinct legacy sibling of same footprint', () => {
  const first = [item(WidgetId.TrafficDown, 0, 0, 2, 2)];
  const second = [item(WidgetId.TrafficUp, 0, 0, 2, 2)];
  const storage = normalizeLayoutStorage(
    { '4x5': first, '12x6': second },
    defaults,
  );
  const edited = [item(WidgetId.TrafficDown, 0, 0, 1, 2)];
  const saved = normalizeLayoutStorage(
    saveLayout(storage, 'legacy:4x5', edited),
    defaults,
  );
  assert.equal(saved.preferredId, 'legacy:4x5');
  assert.deepEqual(saved.entries, [
    { id: 'legacy:4x5', size: { cols: 1, rows: 2 }, items: edited },
    { id: 'legacy:12x6', size: { cols: 2, rows: 2 }, items: second },
  ]);
});

test('v2 loading recomputes stale sizes, removes duplicate entry ids and invalid preference', () => {
  const first = [item(WidgetId.TrafficDown, 0, 0, 4, 3)];
  const storage = normalizeLayoutStorage(
    {
      version: 2,
      preferredId: 'missing',
      entries: [
        { id: 'edited', size: { cols: 1, rows: 1 }, items: first },
        { id: 'edited', size: { cols: 99, rows: 99 }, items: [] },
        { id: 'invalid', items: 'not-an-array' },
      ],
    },
    defaults,
  );
  assert.deepEqual(storage.entries, [
    { id: 'edited', size: { cols: 4, rows: 3 }, items: first },
  ]);
  assert.equal(storage.preferredId, null);
});

test('deliberately empty layouts remain empty through legacy and v2 migration', () => {
  assert.deepEqual(normalizeLayoutStorage({ '4x5': [] }, defaults).entries[0], {
    id: 'legacy:4x5',
    size: { cols: 1, rows: 1 },
    items: [],
  });
  const v2 = normalizeLayoutStorage(
    {
      version: 2,
      preferredId: 'empty',
      entries: [{ id: 'empty', size: { cols: 10, rows: 10 }, items: [] }],
    },
    defaults,
  );
  assert.equal(v2.preferredId, 'empty');
  assert.deepEqual(v2.entries[0]?.items, []);
  assert.deepEqual(v2.entries[0]?.size, { cols: 1, rows: 1 });
});

test('malformed entries are skipped; a complete legacy entry survives', () => {
  const valid = [item(WidgetId.Memory, 0, 0, 2, 2)];
  const storage = normalizeLayoutStorage(
    {
      '4x5': valid,
      'bad-key': valid,
      '8x6': [{ id: 'broken', type: WidgetId.Memory, x: 0 }],
      '9x9': [
        {
          id: WidgetId.Memory,
          type: WidgetId.Memory,
          x: Infinity,
          y: 0,
          w: 2,
          h: 2,
        },
      ],
      '10x10': [
        { id: WidgetId.Memory, type: WidgetId.Memory, x: -1, y: 0, w: 2, h: 2 },
      ],
    },
    defaults,
  );
  assert.deepEqual(storage.entries, [
    { id: 'legacy:4x5', size: { cols: 2, rows: 2 }, items: valid },
  ]);
});

test('old id-only layout items survive and removed/future types cannot corrupt footprint', () => {
  const original = [
    { id: WidgetId.TrafficDown, x: 0, y: 0, w: 2, h: 2 },
    { id: 'unknown-old', type: 'unknown-old', x: 90, y: 90, w: 8, h: 8 },
    {
      id: 'quota-instance',
      type: WidgetId.SubscriptionQuota,
      x: 2,
      y: 0,
      w: 3,
      h: 2,
    },
  ];
  const normalized = normalizeDashboardItems(original);
  assert.deepEqual(normalized, [
    item(WidgetId.TrafficDown, 0, 0, 2, 2),
    {
      id: 'quota-instance',
      type: WidgetId.SubscriptionQuota,
      x: 2,
      y: 0,
      w: 3,
      h: 2,
    },
  ]);
  const storage = normalizeLayoutStorage({ '12x8': original }, defaults);
  assert.deepEqual(storage.entries[0]?.size, { cols: 5, rows: 2 });
  assert.deepEqual(storage.entries[0]?.items, normalized);
});

test('narrow projection hides excess widgets but does not overwrite their source', () => {
  const source = [
    item(WidgetId.TrafficDown, 0, 0, 4, 2),
    item(WidgetId.TrafficUp, 4, 0, 2, 2),
  ];
  const storage = normalizeLayoutStorage({ '6x2': source }, defaults);
  const selected = selectLayoutEntry(storage, { cols: 2, rows: 2 })!;
  const constraints = {
    [WidgetId.TrafficDown]: { minW: 2, minH: 2 },
    [WidgetId.TrafficUp]: { minW: 2, minH: 2 },
  };
  const narrow = adaptLayout(selected.items, { cols: 2, rows: 2 }, constraints);
  assert.equal(narrow.length, 1);
  assert.deepEqual(
    selectLayoutEntry(storage, { cols: 6, rows: 2 })?.items,
    source,
  );
  assert.deepEqual(
    adaptLayout(source, { cols: 6, rows: 2 }, constraints),
    source,
  );
});

test('resize adaptation has no collisions, never mutates source and respects bounds', () => {
  const source = [
    item(WidgetId.TrafficDown, 0, 0, 2, 2),
    item(WidgetId.TrafficUp, 0, 0, 2, 2),
    item(WidgetId.Memory, 0, 0, 2, 2),
  ];
  const original = structuredClone(source);
  const size = { cols: 4, rows: 2 };
  const constraints = Object.fromEntries(
    source.map(({ id }) => [id, { minW: 2, minH: 2 }]),
  );
  const adapted = adaptLayout(source, size, constraints);
  assert.equal(adapted.length, 2);
  for (const widget of adapted) {
    assert.ok(widget.x >= 0 && widget.y >= 0);
    assert.ok(
      widget.x + widget.w <= size.cols && widget.y + widget.h <= size.rows,
    );
    for (const other of adapted.filter(({ id }) => id !== widget.id)) {
      const overlap =
        widget.x < other.x + other.w &&
        widget.x + widget.w > other.x &&
        widget.y < other.y + other.h &&
        widget.y + widget.h > other.y;
      assert.equal(overlap, false);
    }
  }
  assert.deepEqual(source, original);
});

test('invalid v2 entries fall back to defaults without breaking the dashboard', () => {
  const storage = normalizeLayoutStorage(
    { version: 2, preferredId: 'lost', entries: [{}] },
    defaults,
  );
  assert.equal(storage.preferredId, null);
  assert.equal(storage.entries.length, 1);
  assert.equal(storage.entries[0]?.id, 'legacy:4x5');
});

test('an acknowledged edit survives serialized storage reload and restores the preferred layout', () => {
  const initial = normalizeLayoutStorage(
    {
      '6x4': [item(WidgetId.TrafficDown, 0, 0, 2, 2)],
      '12x8': [item(WidgetId.TrafficUp, 0, 0, 2, 2)],
    },
    defaults,
  );
  const preferred = saveLayout(initial, 'legacy:12x8', [
    item(WidgetId.TrafficUp, 0, 0, 4, 3),
    item(WidgetId.Memory, 4, 0, 3, 2),
  ]);
  const restarted = normalizeLayoutStorage(
    JSON.parse(JSON.stringify(preferred)),
    defaults,
  );
  assert.deepEqual(restarted, preferred);
  assert.equal(
    selectLayoutEntry(restarted, { cols: 4, rows: 4 })?.id,
    'legacy:12x8',
  );
  assert.equal(
    selectLayoutEntry(restarted, { cols: 12, rows: 8 })?.id,
    'legacy:12x8',
  );
  assert.deepEqual(
    restarted.entries.find(({ id }) => id === 'legacy:6x4'),
    initial.entries[0],
  );
});

test('restarting after a rejected write loads only the last acknowledged snapshot', () => {
  const acknowledged = normalizeLayoutStorage(
    {
      '6x4': [item(WidgetId.TrafficDown, 0, 0, 3, 2)],
    },
    defaults,
  );
  const failedDraft = saveLayout(acknowledged, 'legacy:6x4', [
    item(WidgetId.TrafficDown, 0, 0, 2, 2),
    item(WidgetId.TrafficUp, 2, 0, 2, 2),
  ]);
  assert.notDeepEqual(failedDraft, acknowledged);
  const reloaded = normalizeLayoutStorage(
    JSON.parse(JSON.stringify(acknowledged)),
    defaults,
  );
  assert.deepEqual(reloaded, acknowledged);
  assert.deepEqual(reloaded.entries[0]?.items, [
    item(WidgetId.TrafficDown, 0, 0, 3, 2),
  ]);
});
