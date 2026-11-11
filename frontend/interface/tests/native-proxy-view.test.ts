import assert from 'node:assert/strict';
import test from 'node:test';
import type { NativeProxies_Serialize } from '../src/ipc/bindings.js';
import {
  CLASH_PROXIES_QUERY_KEY,
  NATIVE_PROXIES_QUERY_KEY,
} from '../src/ipc/consts.js';
import { selectVisibleNativeProxies } from '../src/ipc/use-native-proxies.js';
import { mutationKeys } from '../src/provider/mutation-provider.js';

test('native proxy view excludes hidden groups without duplicating nodes', () => {
  const sharedNode = {
    name: 'Shared node',
    type: 'Vless',
    history: [],
    udp: true,
  };
  const graph = {
    global: {
      name: 'GLOBAL',
      all: ['Shared node'],
      now: 'Shared node',
    },
    groups: [
      { name: 'Visible A', hidden: false, all: ['Shared node'] },
      { name: 'Hidden', hidden: true, all: ['Shared node'] },
      { name: 'Visible B', hidden: false, all: ['Shared node'] },
    ],
    nodes: { 'Shared node': sharedNode },
  } as unknown as NativeProxies_Serialize;

  const selected = selectVisibleNativeProxies(graph);

  assert.deepEqual(
    selected.groups.map((group) => group.name),
    ['Visible A', 'Visible B'],
  );
  assert.strictEqual(selected.nodes, graph.nodes);
  assert.strictEqual(selected.nodes['Shared node'], sharedNode);
  assert.strictEqual(selected.global, graph.global);
  assert.equal(graph.groups.length, 3, 'source graph must remain unchanged');
  assert.equal(
    selected.groups[0].all[0],
    selected.groups[1].all[0],
    'both groups reference the same indexed node by name',
  );
});

test('native proxy view returns the original identity when nothing is hidden', () => {
  const graph = {
    global: null,
    groups: [{ name: 'Visible', hidden: false, all: ['DIRECT'] }],
    nodes: { DIRECT: { name: 'DIRECT', type: 'Direct' } },
  } as unknown as NativeProxies_Serialize;

  assert.strictEqual(selectVisibleNativeProxies(graph), graph);
});

test('runtime inspection caches invalidate together on committed configuration changes', () => {
  for (const kind of [
    'nyanpasu_config',
    'clash_config',
    'profiles',
    'runtime_transform_diagnostics',
  ] as const) {
    const keys = mutationKeys[kind].map((queryKey) => queryKey[0]);
    assert.equal(
      keys.filter((key) => key === 'inspectRuntime').length,
      1,
      `${kind}: exactly one generated snapshot invalidation`,
    );
    assert.equal(
      keys.filter((key) => key === 'inspectAppliedRuntime').length,
      1,
      `${kind}: exactly one last-applied snapshot invalidation`,
    );
  }
});

test('backend state changes invalidate both old and normalized proxy caches', () => {
  assert.notEqual(NATIVE_PROXIES_QUERY_KEY, CLASH_PROXIES_QUERY_KEY);
  for (const kind of [
    'nyanpasu_config',
    'clash_config',
    'profiles',
    'proxies',
  ] as const) {
    const keys = mutationKeys[kind].map((queryKey) => queryKey[0]);
    assert.ok(
      keys.includes(CLASH_PROXIES_QUERY_KEY),
      `${kind}: legacy proxy cache must refresh`,
    );
    assert.ok(
      keys.includes(NATIVE_PROXIES_QUERY_KEY),
      `${kind}: normalized proxy cache must refresh`,
    );
    assert.equal(
      keys.filter((key) => key === NATIVE_PROXIES_QUERY_KEY).length,
      1,
      `${kind}: only one native invalidation per event`,
    );
  }
});
