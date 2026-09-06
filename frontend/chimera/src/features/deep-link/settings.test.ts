import assert from 'node:assert/strict';
import test from 'node:test';
import type { Locale } from '@/paraglide/runtime';
import {
  applySettingsDeepLink,
  type DeepLinkSettingsGateway,
} from './settings';

const createGateway = (initialProxy = false) => {
  let proxy = initialProxy;
  let language: Locale | undefined;
  const proxyWrites: boolean[] = [];
  const languageWrites: Locale[] = [];

  const gateway: DeepLinkSettingsGateway = {
    getSystemProxyEnabled: async () => proxy,
    setSystemProxyEnabled: async (enabled) => {
      proxy = enabled;
      proxyWrites.push(enabled);
    },
    setLanguage: async (locale) => {
      language = locale;
      languageWrites.push(locale);
    },
  };

  return {
    gateway,
    proxy: () => proxy,
    language: () => language,
    proxyWrites,
    languageWrites,
  };
};

test('system proxy on and off are idempotent', async () => {
  const state = createGateway(false);

  assert.equal(
    await applySettingsDeepLink(
      { type: 'system-proxy', mode: 'off' },
      state.gateway,
    ),
    false,
  );
  assert.deepEqual(state.proxyWrites, []);

  assert.equal(
    await applySettingsDeepLink(
      { type: 'system-proxy', mode: 'on' },
      state.gateway,
    ),
    true,
  );
  assert.equal(state.proxy(), true);
  assert.deepEqual(state.proxyWrites, [true]);

  assert.equal(
    await applySettingsDeepLink(
      { type: 'system-proxy', mode: 'on' },
      state.gateway,
    ),
    false,
  );
  assert.deepEqual(state.proxyWrites, [true]);
});

test('system proxy toggle flips on every invocation', async () => {
  const state = createGateway(false);

  await applySettingsDeepLink(
    { type: 'system-proxy', mode: 'toggle' },
    state.gateway,
  );
  await applySettingsDeepLink(
    { type: 'system-proxy', mode: 'toggle' },
    state.gateway,
  );

  assert.equal(state.proxy(), false);
  assert.deepEqual(state.proxyWrites, [true, false]);
});

test('language action persists the exact supported locale', async () => {
  const state = createGateway();

  await applySettingsDeepLink(
    { type: 'language', locale: 'zh-cn' },
    state.gateway,
  );

  assert.equal(state.language(), 'zh-cn');
  assert.deepEqual(state.languageWrites, ['zh-cn']);
});
