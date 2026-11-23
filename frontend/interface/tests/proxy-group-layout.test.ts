import assert from 'node:assert/strict';
import { test } from 'node:test';
import type { ProxyGroupLayout } from '../src/ipc/bindings';
import {
  applyProxyGroupLayout,
  flatProxyGroupOrder,
  resolveProxyGroupLayout,
  storedProxyGroupLayoutOf,
} from '../src/ipc/proxy-group-layout';

const folder = (id: string, name: string, members: string[]) => ({
  id,
  name,
  members,
});
const layout = (
  ungrouped: string[],
  folders: ProxyGroupLayout['folders'] = [],
): ProxyGroupLayout => ({
  ungrouped,
  folders,
});

test('an empty layout uses the live core order and removes duplicate live names', () => {
  assert.deepEqual(
    resolveProxyGroupLayout(undefined, ['B', 'A', 'A', 'C']),
    layout(['B', 'A', 'C']),
  );
  assert.deepEqual(
    resolveProxyGroupLayout(layout([]), ['B', 'A']),
    layout(['B', 'A']),
  );
});

test('stored positions are stable and absent groups are not shown', () => {
  const stored = layout(
    ['Gone', 'A'],
    [folder('f1', 'Streaming', ['Gone', 'D'])],
  );
  const before = structuredClone(stored);
  assert.deepEqual(
    resolveProxyGroupLayout(stored, ['A', 'D', 'X']),
    layout(['A', 'X'], [folder('f1', 'Streaming', ['D'])]),
  );
  assert.deepEqual(stored, before);
});

test('the first placement wins, new groups append after ungrouped', () => {
  const stored = layout(['C', 'C'], [folder('f1', 'Work', ['C', 'B'])]);
  assert.deepEqual(
    resolveProxyGroupLayout(stored, ['B', 'C', 'A']),
    layout(['C', 'A'], [folder('f1', 'Work', ['B'])]),
  );
});

test('flattened sidebar keeps ungrouped before folders in their stored order', () => {
  assert.deepEqual(
    flatProxyGroupOrder(layout(['C'], [folder('f1', 'One', ['D', 'E'])])),
    ['C', 'D', 'E'],
  );
});

test('only the current Config Profile owns its presentation layout', () => {
  const profiles = {
    current: 'b',
    proxy_group_layouts: {
      a: layout(['A']),
      b: layout(['B']),
    },
  };
  assert.deepEqual(storedProxyGroupLayoutOf(profiles), layout(['B']));
  assert.equal(
    storedProxyGroupLayoutOf({ ...profiles, current: 'c' }),
    undefined,
  );
  assert.equal(
    storedProxyGroupLayoutOf({ ...profiles, current: null }),
    undefined,
  );
});

test('optimistic changes preserve other profiles, empty changes remove only their entry', () => {
  const profiles = {
    current: 'a',
    proxy_group_layouts: {
      a: layout(['A']),
      b: layout(['B']),
    },
  };
  const updated = applyProxyGroupLayout(profiles, 'a', layout(['X']));
  assert.deepEqual(updated.proxy_group_layouts, {
    a: layout(['X']),
    b: layout(['B']),
  });
  assert.deepEqual(profiles.proxy_group_layouts.a, layout(['A']));
  assert.deepEqual(
    applyProxyGroupLayout(profiles, 'a', layout([])).proxy_group_layouts,
    { b: layout(['B']) },
  );
});
