import assert from 'node:assert/strict';
import test from 'node:test';
import type { DashboardItem } from '../src/components/widgets/consts.ts';
import { WidgetId } from '../src/components/widgets/widget-config.ts';
import {
  applyDashboardLayoutEdit,
  createDashboardLayoutSaveQueue,
} from '../src/pages/(main)/main/dashboard/_modules/layout-save-model.ts';
import {
  normalizeLayoutStorage,
  type DashboardLayoutStorage,
} from '../src/pages/(main)/main/dashboard/_modules/layout-storage.ts';

const item = (
  id: string,
  type: WidgetId,
  x: number,
  y: number,
): DashboardItem => ({ id, type, x, y, w: 2, h: 2 });
const down = item('down', WidgetId.TrafficDown, 0, 0);
const up = item('up', WidgetId.TrafficUp, 2, 0);
const hidden = item('hidden', WidgetId.Memory, 4, 0);
const other = item('other', WidgetId.Connections, 0, 0);
const base: DashboardLayoutStorage = {
  version: 2,
  preferredId: null,
  entries: [
    { id: 'main', size: { cols: 6, rows: 2 }, items: [down, up, hidden] },
    { id: 'other-layout', size: { cols: 2, rows: 2 }, items: [other] },
  ],
};

const edit = {
  sourceId: 'main',
  visibleIds: ['down', 'up'],
  nextItems: [up, { ...down, x: 0, y: 2 }],
};

test('layout edits retain narrow-screen hidden widgets and other layout entries', () => {
  const saved = applyDashboardLayoutEdit(base, edit);
  assert.equal(saved.preferredId, 'main');
  assert.deepEqual(saved.entries[0]?.items, [...edit.nextItems, hidden]);
  assert.deepEqual(saved.entries[1], base.entries[1]);
  assert.deepEqual(base.entries[0]?.items, [down, up, hidden]);
});

test('replaying the failed intent after remote updates preserves remote hidden widgets', () => {
  const newerHidden = { ...hidden, y: 3 };
  const newlyInserted = item(
    'new-from-other-client',
    WidgetId.RuleTraffic,
    4,
    4,
  );
  const newer = {
    ...base,
    entries: [
      { ...base.entries[0]!, items: [down, up, newerHidden, newlyInserted] },
      base.entries[1]!,
    ],
  };
  const retried = applyDashboardLayoutEdit(newer, edit);
  assert.deepEqual(retried.entries[0]?.items, [
    ...edit.nextItems,
    newerHidden,
    newlyInserted,
  ]);
  assert.deepEqual(retried.entries[1], newer.entries[1]);
});

test('an explicitly removed visible widget stays removed, but hidden widgets remain', () => {
  const edited = applyDashboardLayoutEdit(base, {
    sourceId: 'main',
    visibleIds: ['down', 'up'],
    nextItems: [down],
  });
  assert.deepEqual(edited.entries[0]?.items, [down, hidden]);
});

test('retrying an already-visible failed draft is idempotent for new Widget instances', () => {
  const newlyAdded = item('new-widget', WidgetId.RecentTraffic, 2, 2);
  const insert = {
    sourceId: 'main',
    visibleIds: ['down', 'up'],
    nextItems: [down, up, newlyAdded],
  };
  const first = applyDashboardLayoutEdit(base, insert);
  const retried = applyDashboardLayoutEdit(first, insert);
  assert.deepEqual(retried.entries[0]?.items, [down, up, newlyAdded, hidden]);
  assert.deepEqual(retried.entries, first.entries);
});

test('does not resurrect a layout deleted remotely during a failed write', () => {
  const newer = normalizeLayoutStorage(
    {
      version: 2,
      preferredId: 'other-layout',
      entries: [base.entries[1]],
    },
    {},
  );
  assert.throws(() => applyDashboardLayoutEdit(newer, edit), /was removed/);
  assert.deepEqual(newer.entries, [base.entries[1]]);
});

function deferred<T>() {
  let resolve!: (value: T) => void;
  const promise = new Promise<T>((done) => {
    resolve = done;
  });
  return { promise, resolve };
}

async function flush() {
  await new Promise((resolve) => setImmediate(resolve));
}

test('save status remains pending until KV confirmation and rejects repeated drags', async () => {
  const inFlight = deferred<boolean>();
  const states: string[] = [];
  const writes: string[] = [];
  const queue = createDashboardLayoutSaveQueue({
    write: async (operation) => {
      writes.push(operation.sourceId);
      return inFlight.promise;
    },
    refresh: async () => {},
    onStateChange: (state) => states.push(state),
  });
  assert.equal(queue.save(edit), true);
  assert.equal(queue.getState(), 'saving');
  assert.equal(queue.save({ ...edit, nextItems: [] }), false);
  assert.equal(queue.retry(), false);
  inFlight.resolve(true);
  await flush();
  assert.equal(queue.getState(), 'saved');
  assert.deepEqual(states, ['saving', 'saved']);
  assert.deepEqual(writes, ['main']);
});

test('failed writes stay in error state until a retry commits against newer remote data', async () => {
  const states: string[] = [];
  let current = base;
  let attempts = 0;
  let refreshed = 0;
  const queue = createDashboardLayoutSaveQueue({
    write: async (intent) => {
      attempts++;
      if (attempts === 1) return false;
      current = applyDashboardLayoutEdit(current, intent);
      return true;
    },
    refresh: async () => {
      refreshed++;
      current = {
        ...current,
        entries: [
          { ...current.entries[0]!, items: [down, up, { ...hidden, y: 4 }] },
          current.entries[1]!,
        ],
      };
    },
    onStateChange: (state) => states.push(state),
  });
  assert.equal(queue.save(edit), true);
  await flush();
  assert.equal(queue.getState(), 'error');
  assert.equal(refreshed, 1);
  assert.equal(queue.save({ ...edit, nextItems: [] }), false);
  assert.equal(queue.retry(), true);
  await flush();
  assert.equal(queue.getState(), 'saved');
  assert.deepEqual(current.entries[0]?.items, [
    ...edit.nextItems,
    { ...hidden, y: 4 },
  ]);
  assert.deepEqual(current.entries[1], base.entries[1]);
  assert.deepEqual(states, ['saving', 'error', 'saving', 'saved']);
});

test('exceptions and refresh failures display error and unlock retry', async () => {
  let attempts = 0;
  const queue = createDashboardLayoutSaveQueue({
    write: async () => {
      attempts++;
      if (attempts === 1) throw new Error('offline');
      return true;
    },
    refresh: async () => {
      throw new Error('offline read');
    },
    onStateChange: () => {},
  });
  queue.save(edit);
  await flush();
  assert.equal(queue.getState(), 'error');
  assert.equal(queue.retry(), true);
  await flush();
  assert.equal(queue.getState(), 'saved');
  assert.equal(attempts, 2);
});

test('a failed save remains pending until the refreshed authoritative snapshot settles', async () => {
  const pendingRefresh = deferred<void>();
  const states: string[] = [];
  const queue = createDashboardLayoutSaveQueue({
    write: async () => false,
    refresh: () => pendingRefresh.promise,
    onStateChange: (state) => states.push(state),
  });
  queue.save(edit);
  await flush();
  assert.equal(queue.getState(), 'saving');
  assert.equal(queue.retry(), false);
  pendingRefresh.resolve();
  await flush();
  assert.equal(queue.getState(), 'error');
  assert.deepEqual(states, ['saving', 'error']);
});

test('rapid consecutive edits serialize, then restart from the last acknowledged write', async () => {
  let committed = base;
  const firstAck = deferred<boolean>();
  let writes = 0;
  const saver = createDashboardLayoutSaveQueue({
    write: async (intent) => {
      writes++;
      if (writes === 1 && !(await firstAck.promise)) return false;
      committed = applyDashboardLayoutEdit(committed, intent);
      return true;
    },
    refresh: async () => {},
    onStateChange: () => {},
  });
  const move = { ...edit, nextItems: [{ ...down, x: 1 }, up] };
  const remove = {
    sourceId: 'main',
    visibleIds: ['down', 'up'],
    nextItems: [up],
  };
  assert.equal(saver.save(move), true);
  assert.equal(saver.save(remove), false);
  assert.equal(writes, 1);
  firstAck.resolve(true);
  await flush();
  assert.equal(saver.getState(), 'saved');
  assert.equal(saver.save(remove), true);
  await flush();
  assert.equal(writes, 2);
  assert.equal(saver.getState(), 'saved');
  const reloaded = normalizeLayoutStorage(
    JSON.parse(JSON.stringify(committed)),
    {},
  );
  assert.deepEqual(reloaded.entries[0]?.items, [up, hidden]);
  assert.equal(reloaded.preferredId, 'main');
  assert.deepEqual(reloaded.entries[1], base.entries[1]);
});
