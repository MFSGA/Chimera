import assert from 'node:assert/strict';
import test from 'node:test';
import type {
  NativeProxies_Serialize,
  Proxy_Serialize,
} from '../src/ipc/bindings.js';
import { withNativeDelaySample } from '../src/ipc/use-native-proxy-actions.js';

function graph(): NativeProxies_Serialize {
  const sharedNode = {
    name: 'Shared',
    type: 'Vless',
    udp: true,
    history: [{ time: 'old', delay: 12 }],
    extra: {
      'https://example.test/': {
        alive: true,
        history: [{ time: 'older', delay: 17 }],
      },
    },
  } as Proxy_Serialize;
  const otherNode = {
    name: 'DIRECT',
    type: 'Direct',
    udp: false,
    history: [],
  } as Proxy_Serialize;
  return {
    global: null,
    groups: [
      { name: 'A', all: ['Shared'], hidden: false },
      { name: 'B', all: ['Shared'], hidden: false },
    ],
    nodes: { Shared: sharedNode, DIRECT: otherNode },
  } as unknown as NativeProxies_Serialize;
}

test('native latency result updates one indexed node without copying shared group membership', () => {
  const previous = graph();
  const updated = withNativeDelaySample(previous, 'Shared', 48, null)!;
  assert.notStrictEqual(updated, previous);
  assert.notStrictEqual(updated.nodes, previous.nodes);
  assert.notStrictEqual(updated.nodes.Shared, previous.nodes.Shared);
  assert.strictEqual(updated.nodes.DIRECT, previous.nodes.DIRECT);
  assert.strictEqual(updated.groups, previous.groups);
  assert.strictEqual(updated.global, previous.global);
  assert.deepEqual(
    updated.groups.map((group) => group.all[0]),
    ['Shared', 'Shared'],
  );
  assert.deepEqual(
    updated.nodes.Shared.history.map((item) => item.delay),
    [12, 48],
  );
  assert.deepEqual(
    previous.nodes.Shared.history.map((item) => item.delay),
    [12],
  );
  assert.strictEqual(updated.nodes.Shared.extra, previous.nodes.Shared.extra);
});

test('native latency result updates URL-specific history and marks zero latency unavailable', () => {
  const previous = graph();
  const url = 'https://example.test/';
  const updated = withNativeDelaySample(previous, 'Shared', 0, url)!;
  assert.equal(updated.nodes.Shared.extra?.[url]?.alive, false);
  assert.deepEqual(
    updated.nodes.Shared.extra?.[url]?.history.map((item) => item.delay),
    [17, 0],
  );
  assert.equal(
    previous.nodes.Shared.extra?.[url]?.history.length,
    1,
    'the cached previous graph must remain immutable',
  );
  assert.equal(updated.nodes.Shared.history.at(-1)?.delay, 0);
});

test('missing native node or missing snapshot preserves identity', () => {
  const previous = graph();
  assert.strictEqual(
    withNativeDelaySample(previous, 'not-in-snapshot', 42, null),
    previous,
  );
  assert.equal(withNativeDelaySample(undefined, 'Shared', 42, null), undefined);
});
