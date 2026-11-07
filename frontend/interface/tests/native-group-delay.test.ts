import assert from 'node:assert/strict';
import test from 'node:test';
import type { NativeProxies_Serialize } from '../src/ipc/bindings.js';
import {
  PINNED_GROUP_DELAY_CONCURRENCY,
  testNativeGroupDelay,
  type NativeGroupDelayApi,
} from '../src/ipc/native-group-delay.js';

function graph({
  fixed = null,
  names = ['node-a', 'node-b'],
  groupName = 'Auto',
  url = 'https://example.test/generate_204',
  expected = '200/204',
}: {
  fixed?: string | null;
  names?: string[];
  groupName?: string;
  url?: string | null;
  expected?: string | null;
} = {}): NativeProxies_Serialize {
  return {
    global: null,
    groups: [
      {
        name: groupName,
        all: names,
        fixed,
        testUrl: url,
        expectedStatus: expected,
      },
    ],
    nodes: Object.fromEntries(
      names.map((name, index) => [
        name,
        index % 2 === 0
          ? { name, provider: 'Provider A' }
          : { name, 'provider-name': 'Provider B' },
      ]),
    ),
  } as unknown as NativeProxies_Serialize;
}

test('latest unpinned group uses one group test with its URL and expected status', async () => {
  const calls: string[] = [];
  const api: NativeGroupDelayApi = {
    refresh: async () => {
      calls.push('read');
      return graph();
    },
    groupDelay: async (group, url, expected) => {
      calls.push(`group:${group}:${url}:${expected}`);
      return { 'node-a': 19 };
    },
    proxyDelay: async () => {
      throw new Error('unpinned group must use its group endpoint');
    },
  };

  const delays = await testNativeGroupDelay('Auto', api);
  assert.deepEqual(delays, { 'node-a': 19 });
  assert.deepEqual(calls, [
    'read',
    'group:Auto:https://example.test/generate_204:200/204',
  ]);
});

test('newly pinned group tests all members with provider routes, at most eight at a time', async () => {
  const names = Array.from({ length: 23 }, (_, index) => `node-${index}`);
  let active = 0;
  let peak = 0;
  let refreshes = 0;
  let groupCalls = 0;
  const requests: Array<{
    name: string;
    provider: string | null;
    url: string | null;
    expected: string | null;
  }> = [];
  const api: NativeGroupDelayApi = {
    refresh: async () => {
      refreshes += 1;
      // The view cached before this action may have been unpinned.
      return graph({ fixed: 'node-0', names });
    },
    groupDelay: async () => {
      groupCalls += 1;
      return {};
    },
    proxyDelay: async (name, provider, url, expected) => {
      active += 1;
      peak = Math.max(peak, active);
      requests.push({ name, provider, url, expected });
      await new Promise<void>((resolve) => setTimeout(resolve, 2));
      active -= 1;
      if (name === 'node-11') {
        throw new Error('one controller healthcheck failed');
      }
      return Number(name.slice('node-'.length)) + 1;
    },
  };

  const delays = await testNativeGroupDelay('Auto', api);
  assert.equal(refreshes, 1);
  assert.equal(groupCalls, 0, 'pinned groups must not call group-delay');
  assert.ok(peak > 1);
  assert.ok(peak <= PINNED_GROUP_DELAY_CONCURRENCY);
  assert.equal(
    requests.length,
    names.length,
    'no test is automatically retried',
  );
  assert.deepEqual(
    requests.map((request) => request.name),
    names,
    'every member is dispatched once in source order',
  );
  assert.equal(delays['node-0'], 1);
  assert.equal(delays['node-11'], 0, 'failed member must not abort the rest');
  assert.equal(delays['node-22'], 23);
  for (const [index, request] of requests.entries()) {
    assert.equal(request.provider, index % 2 ? 'Provider B' : 'Provider A');
    assert.equal(request.url, 'https://example.test/generate_204');
    assert.equal(request.expected, '200/204');
  }
});

test('controller snapshot failure or missing group never guesses the pin state', async () => {
  let requests = 0;
  const api: NativeGroupDelayApi = {
    refresh: async () => graph(),
    groupDelay: async () => {
      requests += 1;
      return {};
    },
    proxyDelay: async () => {
      requests += 1;
      return 5;
    },
  };

  await assert.rejects(
    () => testNativeGroupDelay('Missing', api),
    /missing from the current controller/,
  );
  await assert.rejects(
    () =>
      testNativeGroupDelay('Auto', {
        ...api,
        refresh: async () => {
          throw new Error('cannot inspect the running core');
        },
      }),
    /cannot inspect the running core/,
  );
  assert.equal(requests, 0);
});

test('a pinned group with no members does not invoke the group endpoint', async () => {
  const api: NativeGroupDelayApi = {
    refresh: async () => graph({ fixed: 'unknown', names: [] }),
    groupDelay: async () => {
      throw new Error('must not clear the pin');
    },
    proxyDelay: async () => {
      throw new Error('there are no members');
    },
  };
  assert.deepEqual(await testNativeGroupDelay('Auto', api), {});
});
