import assert from 'node:assert/strict';
import test from 'node:test';
import {
  collectDelayKeys,
  groupDelayKey,
  latestDelay,
  nativeMemberDelay,
  nativeMemberState,
  nodeDelayHistory,
  resolveProxyChain,
  selectVisibleNativeProxies,
  type Proxies_Serialize,
  type Proxy_Serialize,
  type ProxyGroup,
} from '@chimera/interface';

const node = (name: string, delays: number[] = []): Proxy_Serialize =>
  ({
    name,
    type: 'Direct',
    udp: false,
    history: delays.map((delay) => ({ time: '', delay })),
    now: null,
    all: null,
  }) as Proxy_Serialize;

const groupRecord = (name: string, now: string | null, all: string[]) => ({
  ...node(name),
  type: 'Selector',
  now,
  all,
});

const group = (
  name: string,
  now: string | null,
  all: string[],
  delayKey = '',
  hidden = false,
) =>
  ({
    name,
    type: 'Selector',
    now,
    all,
    delayKey,
    hidden,
  }) as ProxyGroup;

const snapshot = (
  groups: ProxyGroup[],
  nodes: Record<string, Proxy_Serialize>,
  global: ProxyGroup | null = null,
): Proxies_Serialize => ({ groups, nodes, global });

const sample = (delay: number, time = '') => ({
  alive: delay > 0,
  history: [{ time, delay }],
});

test('delay sources are opaque keys with the same fallback as ref', () => {
  const base = node('n', [10, 20]);
  const withExtra = {
    ...base,
    extra: { '': sample(15), specific: sample(99) },
  };
  assert.equal(latestDelay(withExtra, 'specific'), 99);
  assert.equal(latestDelay(withExtra, ''), 15);
  assert.equal(latestDelay(withExtra, 'other'), 20);
  assert.equal(latestDelay({ ...base, extra: {} }, 'specific'), 20);
  assert.equal(latestDelay(base, 'specific'), 20);
  assert.equal(latestDelay(undefined, 'specific'), undefined);
  assert.strictEqual(nodeDelayHistory(base, 'specific'), base.history);
});

test('group delay key map does not mistake inherited Object names for groups', () => {
  const own = group('own', null, [], 'own-source');
  const standard = group('standard', null, []);
  const graph = snapshot([own, standard], {});
  assert.deepEqual(collectDelayKeys(graph), {
    own: 'own-source',
    standard: '',
  });
  assert.equal(groupDelayKey(graph, 'own'), 'own-source');
  assert.equal(groupDelayKey(graph, 'standard'), '');
  assert.equal(groupDelayKey(graph, 'toString'), '');
  assert.equal(groupDelayKey(undefined, 'own'), '');
});

test('selection follows a nested group down to its leaf', () => {
  const graph = snapshot([], {
    outer: groupRecord('outer', 'inner', ['inner']),
    inner: groupRecord('inner', 'leaf', ['leaf']),
    leaf: node('leaf', [30]),
  });
  assert.deepEqual(resolveProxyChain('outer', graph), {
    path: ['inner', 'leaf'],
    leaf: graph.nodes.leaf,
    parent: 'inner',
  });
});

test('missing members, unselected groups and cycles do not invent a leaf or delay', () => {
  const graph = snapshot([group('root', 'missing', [])], {
    root: groupRecord('root', 'inner', ['inner']),
    inner: groupRecord('inner', 'root', ['root']),
  });
  assert.equal(
    nativeMemberState('missing', graph.groups[0], graph).delay,
    undefined,
  );
  assert.equal(
    nativeMemberState('inner', graph.groups[0], graph).delay,
    undefined,
  );
  assert.equal(resolveProxyChain('root', graph).leaf, undefined);
  const missing = snapshot([], { a: groupRecord('a', 'gone', ['gone']) });
  assert.deepEqual(resolveProxyChain('a', missing), {
    path: ['gone'],
    parent: 'a',
  });
  const inactive = snapshot([], { a: groupRecord('a', null, []) });
  assert.deepEqual(resolveProxyChain('a', inactive), { path: [], parent: 'a' });
});

test('nested member uses containing leaf group healthcheck source and keeps history identity', () => {
  const inner = group('inner', 'leaf', ['leaf'], 'inner-key');
  const outer = group('outer', 'inner', ['inner'], 'outer-key');
  const innerHistory = [{ time: '', delay: 42 }];
  const leaf = {
    ...node('leaf', [70]),
    extra: {
      'inner-key': { alive: true, history: innerHistory },
      '': sample(99),
    },
  };
  const graph = snapshot([outer, inner], {
    inner: groupRecord('inner', 'leaf', ['leaf']),
    leaf,
    direct: node('direct', [5]),
  });
  const state = nativeMemberState('inner', outer, graph);
  assert.equal(state.leaf, 'leaf');
  assert.equal(state.delay, 42);
  assert.strictEqual(state.history, innerHistory);
  const direct = nativeMemberState('direct', outer, graph);
  assert.equal(direct.delay, 5);
  assert.strictEqual(direct.history, graph.nodes.direct.history);
  assert.equal(nativeMemberDelay('inner', outer, graph), 42);
});

test('filtered hidden group still supplies its own source key to nested members', () => {
  const hidden = group('hidden', 'leaf', ['leaf'], 'hidden-source', true);
  const outer = group('outer', 'hidden', ['hidden']);
  const graph = snapshot([outer, hidden], {
    hidden: groupRecord('hidden', 'leaf', ['leaf']),
    leaf: {
      ...node('leaf', [70]),
      extra: { 'hidden-source': sample(40) },
    },
  });
  const visible = selectVisibleNativeProxies(graph);
  assert.deepEqual(
    visible.groups.map((item) => item.name),
    ['outer'],
  );
  assert.equal(groupDelayKey(visible, 'hidden'), 'hidden-source');
  assert.equal(nativeMemberState('hidden', outer, visible).delay, 40);
  assert.equal(graph.groups.length, 2);
});

test('a shared leaf uses different healthcheck source for each group', () => {
  const leaf = {
    ...node('leaf', [1]),
    extra: { '': sample(10), a: sample(20), b: sample(30) },
  };
  const graph = snapshot(
    [
      group('defaults', 'leaf', ['leaf']),
      group('a', 'leaf', ['leaf'], 'a'),
      group('b', 'leaf', ['leaf'], 'b'),
    ],
    { leaf },
  );
  assert.equal(nativeMemberDelay('leaf', graph.groups[0], graph), 10);
  assert.equal(nativeMemberDelay('leaf', graph.groups[1], graph), 20);
  assert.equal(nativeMemberDelay('leaf', graph.groups[2], graph), 30);
});

test('failure 0, untested undefined, and newer samples remain distinguishable', () => {
  const selected = node('selected', [30, 80]);
  const root = group('root', 'selected', ['selected']);
  const graph = snapshot([root], { selected });
  assert.equal(nativeMemberDelay('selected', root, graph), 80);
  selected.history.push({ time: '', delay: 0 });
  assert.equal(nativeMemberDelay('selected', root, graph), 0);
  selected.history = [];
  assert.equal(nativeMemberDelay('selected', root, graph), undefined);
});
