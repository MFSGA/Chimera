import assert from 'node:assert/strict';
import test from 'node:test';
import { commands, type NativeProxies_Serialize } from '../src/ipc/bindings.js';
import {
  CLASH_CONFIG_QUERY_KEY,
  CLASH_PROXIES_PROVIDER_QUERY_KEY,
  CLASH_PROXIES_QUERY_KEY,
  CLASH_RULES_PROVIDER_QUERY_KEY,
  CLASH_RULES_QUERY_KEY,
  CLASH_VERSION_QUERY_KEY,
} from '../src/ipc/consts.js';
import { mutations, queries } from '../src/ipc/query-bindings.js';
import {
  invokeMutation,
  MutationUnconfirmedError,
  unwrapQueryOptions,
} from '../src/ipc/query-options.js';
import { unwrapResult } from '../src/utils/index.js';

/**
 * Contract CLASH-QUERY-BINDINGS (unit): generated controller commands are
 * classified into cached queries and explicit mutations. The query descriptors
 * preserve existing main/legacy invalidation keys and unwrap typed responses.
 * Provider writes submit once and an unknown transport outcome is unconfirmed.
 * These tests mock the command boundary; they do not exercise a WebView/core.
 */
test('proxy view and delay descriptors keep typed keys and arguments', async () => {
  const oldProxies = commands.getProxies;
  const oldDelay = commands.clashApiGetProxyDelay;
  const oldGroupDelay = commands.clashApiGetGroupDelay;
  const calls: string[] = [];
  commands.getProxies = async () => {
    calls.push('proxies');
    return {
      status: 'ok',
      data: {
        groups: [],
        global: { name: 'GLOBAL' },
        direct: { name: 'DIRECT' },
      },
    } as Awaited<ReturnType<typeof commands.getProxies>>;
  };
  commands.clashApiGetProxyDelay = async (name, url, provider, expected) => {
    calls.push(`node:${name}:${url}:${provider}:${expected}`);
    return { status: 'ok', data: { delay: 42 } };
  };
  commands.clashApiGetGroupDelay = async (group, url, expected) => {
    calls.push(`group:${group}:${url}:${expected}`);
    return { status: 'ok', data: { DIRECT: 42 } };
  };
  try {
    const proxies = queries.getProxies();
    const delay = queries.clashApiGetProxyDelay('DIRECT', null, null, null);
    const providerDelay = queries.clashApiGetProxyDelay(
      'Shared node',
      'https://example.test/check',
      'Provider A',
      '200/204',
    );
    const group = queries.clashApiGetGroupDelay(
      'GLOBAL',
      'https://example.test/',
      null,
    );
    assert.deepEqual(proxies.queryKey, [CLASH_PROXIES_QUERY_KEY]);
    assert.deepEqual(delay.queryKey, [
      'clashApiGetProxyDelay',
      'DIRECT',
      null,
      null,
      null,
    ]);
    assert.deepEqual(providerDelay.queryKey, [
      'clashApiGetProxyDelay',
      'Shared node',
      'https://example.test/check',
      'Provider A',
      '200/204',
    ]);
    assert.deepEqual(group.queryKey, [
      'clashApiGetGroupDelay',
      'GLOBAL',
      'https://example.test/',
      null,
    ]);
    assert.equal(
      (await unwrapQueryOptions(proxies, proxies.queryFn!).queryFn()).global
        .name,
      'GLOBAL',
    );
    assert.equal(
      (await unwrapQueryOptions(delay, delay.queryFn!).queryFn()).delay,
      42,
    );
    assert.equal(
      (
        await unwrapQueryOptions(
          providerDelay,
          providerDelay.queryFn!,
        ).queryFn()
      ).delay,
      42,
    );
    assert.deepEqual(
      await unwrapQueryOptions(group, group.queryFn!).queryFn(),
      { DIRECT: 42 },
    );
    assert.deepEqual(calls, [
      'proxies',
      'node:DIRECT:null:null:null',
      'node:Shared node:https://example.test/check:Provider A:200/204',
      'group:GLOBAL:https://example.test/:null',
    ]);
  } finally {
    commands.getProxies = oldProxies;
    commands.clashApiGetProxyDelay = oldDelay;
    commands.clashApiGetGroupDelay = oldGroupDelay;
  }
});

test('native proxy graph uses a distinct typed query cache and preserves shared node references', async () => {
  const previous = commands.getNativeProxies;
  const graph = {
    global: null,
    groups: [
      { name: 'A', all: ['shared'] },
      { name: 'B', all: ['shared'] },
    ],
    nodes: { shared: { name: 'shared', type: 'Direct' } },
  } as unknown as NativeProxies_Serialize;
  let calls = 0;
  commands.getNativeProxies = async () => {
    calls++;
    return { status: 'ok', data: graph };
  };
  try {
    const native = queries.getNativeProxies();
    assert.deepEqual(native.queryKey, ['getNativeProxies']);
    assert.notDeepEqual(native.queryKey, queries.getProxies().queryKey);
    const result = await unwrapQueryOptions(native, native.queryFn!).queryFn();
    assert.equal(result.groups[0].all[0], result.groups[1].all[0]);
    assert.equal(result.nodes.shared.name, 'shared');
    assert.equal(calls, 1);
  } finally {
    commands.getNativeProxies = previous;
  }
});

test('native proxy graph query rejects domain and transport failures instead of caching them as data', async () => {
  const previous = commands.getNativeProxies;
  try {
    commands.getNativeProxies = async () =>
      ({
        status: 'error',
        error: 'core unavailable',
      }) as Awaited<ReturnType<typeof commands.getNativeProxies>>;
    const domain = queries.getNativeProxies();
    await assert.rejects(unwrapQueryOptions(domain, domain.queryFn!).queryFn());

    commands.getNativeProxies = async () => {
      throw new Error('IPC disconnected');
    };
    const transport = queries.getNativeProxies();
    await assert.rejects(
      unwrapQueryOptions(transport, transport.queryFn!).queryFn(),
      /IPC disconnected/,
    );
  } finally {
    commands.getNativeProxies = previous;
  }
});

test('proxy selection is an explicit write and a lost reply remains unconfirmed', async () => {
  const oldSelect = commands.selectProxy;
  const calls: string[] = [];
  commands.selectProxy = async (group, name) => {
    calls.push(`${group}:${name}`);
    return { status: 'ok', data: null };
  };
  try {
    assert.deepEqual(mutations.selectProxy.mutationKey, ['selectProxy']);
    assert.equal(
      unwrapResult(
        await invokeMutation(mutations.selectProxy, ['GLOBAL', 'DIRECT']),
      ),
      null,
    );
    assert.deepEqual(calls, ['GLOBAL:DIRECT']);
    commands.selectProxy = async () => {
      calls.push('lost');
      throw new Error('IPC reply lost');
    };
    await assert.rejects(
      invokeMutation(mutations.selectProxy, ['GLOBAL', 'REJECT']),
      (error: unknown) =>
        error instanceof MutationUnconfirmedError &&
        error.cause instanceof Error &&
        error.cause.message === 'IPC reply lost',
    );
    assert.deepEqual(calls, ['GLOBAL:DIRECT', 'lost']);
  } finally {
    commands.selectProxy = oldSelect;
  }
});

test('rules and version query descriptors preserve keys and unwrap typed results', async () => {
  const oldRules = commands.clashApiGetRules;
  const oldVersion = commands.clashApiGetVersion;
  const calls: string[] = [];
  commands.clashApiGetRules = async () => {
    calls.push('rules');
    return {
      status: 'ok',
      data: {
        rules: [{ type: 'DOMAIN', payload: 'example.test', proxy: 'DIRECT' }],
      },
    };
  };
  commands.clashApiGetVersion = async () => {
    calls.push('version');
    return {
      status: 'ok',
      data: { version: '1.2.3', premium: null, meta: false },
    };
  };
  try {
    const rules = queries.clashApiGetRules();
    const version = queries.clashApiGetVersion();
    assert.deepEqual(rules.queryKey, [CLASH_RULES_QUERY_KEY]);
    assert.deepEqual(version.queryKey, [CLASH_VERSION_QUERY_KEY]);
    assert.equal(
      (await unwrapQueryOptions(rules, rules.queryFn!).queryFn()).rules[0]
        .payload,
      'example.test',
    );
    assert.equal(
      (await unwrapQueryOptions(version, version.queryFn!).queryFn()).version,
      '1.2.3',
    );
    assert.deepEqual(calls, ['rules', 'version']);
  } finally {
    commands.clashApiGetRules = oldRules;
    commands.clashApiGetVersion = oldVersion;
  }
});

test('provider queries retain invalidation keys; provider updates are explicit mutations', async () => {
  const oldRules = commands.clashApiGetProvidersRules;
  const oldProxies = commands.clashApiGetProvidersProxies;
  const oldRuleUpdate = commands.clashApiUpdateProvidersRules;
  const oldProxyUpdate = commands.updateProxyProvider;
  const calls: string[] = [];
  commands.clashApiGetProvidersRules = async () => {
    calls.push('read-rules');
    return { status: 'ok', data: { providers: {} } };
  };
  commands.clashApiGetProvidersProxies = async () => {
    calls.push('read-proxies');
    return { status: 'ok', data: {} };
  };
  commands.clashApiUpdateProvidersRules = async (name) => {
    calls.push(`update-rule:${name}`);
    return { status: 'ok', data: null };
  };
  commands.updateProxyProvider = async (name) => {
    calls.push(`update-proxy:${name}`);
    return { status: 'ok', data: null };
  };
  try {
    const rules = queries.clashApiGetProvidersRules();
    const proxies = queries.clashApiGetProvidersProxies();
    assert.deepEqual(rules.queryKey, [CLASH_RULES_PROVIDER_QUERY_KEY]);
    assert.deepEqual(proxies.queryKey, [CLASH_PROXIES_PROVIDER_QUERY_KEY]);
    assert.deepEqual(
      await unwrapQueryOptions(rules, rules.queryFn!).queryFn(),
      {
        providers: {},
      },
    );
    assert.deepEqual(
      await unwrapQueryOptions(proxies, proxies.queryFn!).queryFn(),
      {},
    );
    assert.deepEqual(mutations.clashApiUpdateProvidersRules.mutationKey, [
      'clashApiUpdateProvidersRules',
    ]);
    assert.deepEqual(mutations.updateProxyProvider.mutationKey, [
      'updateProxyProvider',
    ]);
    assert.equal(
      unwrapResult(
        await invokeMutation(mutations.clashApiUpdateProvidersRules, ['rules']),
      ),
      null,
    );
    assert.equal(
      unwrapResult(
        await invokeMutation(mutations.updateProxyProvider, ['proxies']),
      ),
      null,
    );
    assert.deepEqual(calls, [
      'read-rules',
      'read-proxies',
      'update-rule:rules',
      'update-proxy:proxies',
    ]);
  } finally {
    commands.clashApiGetProvidersRules = oldRules;
    commands.clashApiGetProvidersProxies = oldProxies;
    commands.clashApiUpdateProvidersRules = oldRuleUpdate;
    commands.updateProxyProvider = oldProxyUpdate;
  }
});

test('lost provider mutation reply is unconfirmed; no automatic retry', async () => {
  const oldUpdate = commands.updateProxyProvider;
  let attempts = 0;
  commands.updateProxyProvider = async () => {
    attempts += 1;
    throw new Error('IPC reply lost');
  };
  try {
    await assert.rejects(
      invokeMutation(mutations.updateProxyProvider, ['provider-a']),
      (error: unknown) =>
        error instanceof MutationUnconfirmedError &&
        error.cause instanceof Error &&
        error.cause.message === 'IPC reply lost',
    );
    assert.equal(attempts, 1);
  } finally {
    commands.updateProxyProvider = oldUpdate;
  }
});

test('controller config query follows ref DTO and runtime patch is an explicit mutation', async () => {
  const oldConfig = commands.clashApiGetConfigs;
  const oldPatch = commands.patchRuntimeOverrides;
  const calls: string[] = [];
  commands.clashApiGetConfigs = async () => {
    calls.push('read-config');
    return {
      status: 'ok',
      data: {
        port: null,
        mode: 'rule',
        ipv6: false,
        'socket-port': null,
        'allow-lan': false,
        'log-level': 'warning',
        'mixed-port': 7890,
        'redir-port': null,
        'socks-port': null,
        'tproxy-port': null,
        'external-controller': null,
        secret: null,
      },
    };
  };
  commands.patchRuntimeOverrides = async (patch) => {
    calls.push(`patch-mode:${patch.mode}`);
    return { status: 'ok', data: null };
  };
  try {
    const config = queries.clashApiGetConfigs();
    assert.deepEqual(config.queryKey, [CLASH_CONFIG_QUERY_KEY]);
    const data = await unwrapQueryOptions(config, config.queryFn!).queryFn();
    assert.equal(data['mixed-port'], 7890);
    assert.equal(data.mode, 'rule');
    assert.equal(data.ipv6, false);
    assert.deepEqual(mutations.patchRuntimeOverrides.mutationKey, [
      'patchRuntimeOverrides',
    ]);
    assert.equal(
      unwrapResult(
        await invokeMutation(mutations.patchRuntimeOverrides, [
          { mode: 'global' },
        ]),
      ),
      null,
    );
    assert.deepEqual(calls, ['read-config', 'patch-mode:global']);
  } finally {
    commands.clashApiGetConfigs = oldConfig;
    commands.patchRuntimeOverrides = oldPatch;
  }
});

test('runtime patch with missing IPC reply must not be automatically retried', async () => {
  const oldPatch = commands.patchRuntimeOverrides;
  let attempts = 0;
  commands.patchRuntimeOverrides = async () => {
    attempts += 1;
    throw new Error('IPC reply lost');
  };
  try {
    await assert.rejects(
      invokeMutation(mutations.patchRuntimeOverrides, [{ mode: 'rule' }]),
      (error: unknown) =>
        error instanceof MutationUnconfirmedError &&
        error.cause instanceof Error &&
        error.cause.message === 'IPC reply lost',
    );
    assert.equal(attempts, 1);
  } finally {
    commands.patchRuntimeOverrides = oldPatch;
  }
});

test('clear fixed sends one mutation and a missing reply stays unconfirmed', async () => {
  const previous = commands.clearProxyFixed;
  const calls: string[] = [];
  commands.clearProxyFixed = async (group) => {
    calls.push(group);
    return { status: 'ok', data: null };
  };
  try {
    const action = mutations.clearProxyFixed;
    assert.deepEqual(action.mutationKey, ['clearProxyFixed']);
    assert.equal(unwrapResult(await invokeMutation(action, ['Auto'])), null);
    assert.deepEqual(calls, ['Auto']);

    commands.clearProxyFixed = async (group) => {
      calls.push(group);
      throw new Error('lost IPC reply after DELETE');
    };
    await assert.rejects(
      () => invokeMutation(action, ['Auto']),
      (error: unknown) => error instanceof MutationUnconfirmedError,
    );
    assert.deepEqual(
      calls,
      ['Auto', 'Auto'],
      'one dispatch per explicit action',
    );
  } finally {
    commands.clearProxyFixed = previous;
  }
});
