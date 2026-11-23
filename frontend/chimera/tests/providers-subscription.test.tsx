import assert from 'node:assert/strict';
import test from 'node:test';
import type { ClashProxiesProviderQueryItem } from '@chimera/interface';
import { createElement } from 'react';
import { renderToStaticMarkup } from 'react-dom/server';
import { useProxiesSubscription } from '../src/pages/(main)/main/providers/_modules/use-proxies-subscription';
import { getLocale, overwriteGetLocale } from '../src/paraglide/runtime';
import { formatBytes } from '../src/utils/format-bytes';
import { informationUnitSymbols } from '../src/utils/information-unit';

type Subscription = NonNullable<
  ClashProxiesProviderQueryItem['subscriptionInfo']
>;

const usage = (subscriptionInfo: Subscription | null) => {
  const data = {
    name: 'offline-fixture',
    type: 'Proxy',
    vehicleType: 'HTTP',
    updatedAt: null,
    proxyCount: 0,
    subscriptionInfo,
  } as ClashProxiesProviderQueryItem;
  let result: ReturnType<typeof useProxiesSubscription> | undefined;
  const Consumer = () => {
    result = useProxiesSubscription(data);
    return null;
  };
  renderToStaticMarkup(createElement(Consumer));
  assert.ok(result, 'The hook must run during server rendering.');
  return result;
};

test('provider without a subscription reports no usage', () => {
  assert.deepEqual(usage(null), {
    progress: 0,
    total: 0,
    used: 0,
    hasSubscriptionInfo: false,
  });
});

test('subscription usage follows ref Upload/Download/Total semantics', () => {
  assert.deepEqual(usage({ Upload: 10, Download: 20, Total: 120, Expire: 0 }), {
    progress: 25,
    total: 120,
    used: 30,
    hasSubscriptionInfo: true,
  });
});

test('negative counters are clamped and over-quota percentage is limited to 100', () => {
  assert.deepEqual(
    usage({ Upload: -10, Download: 170, Total: 100, Expire: 0 }),
    {
      progress: 100,
      total: 100,
      used: 170,
      hasSubscriptionInfo: true,
    },
  );
});

test('zero quota does not divide by zero while subscription remains visible', () => {
  assert.deepEqual(usage({ Upload: 4, Download: 5, Total: -100, Expire: 0 }), {
    progress: 0,
    total: 0,
    used: 9,
    hasSubscriptionInfo: true,
  });
});

test('the ref byte formatter is shared by list and subscription detail', () => {
  // Paraglide requires a locale context in Node. Use its public test seam,
  // then restore the normal resolver so this does not affect other cases.
  const resolveLocale = getLocale;
  overwriteGetLocale(() => 'en');
  try {
    const symbols = informationUnitSymbols();
    assert.ok(symbols.B);
    assert.ok(symbols.KiB);
    assert.equal(formatBytes(0), `0 ${symbols.B}`);
    assert.equal(formatBytes(1024), `1 ${symbols.KiB}`);
    assert.equal(formatBytes(1024 * 1024), `1 ${symbols.MiB}`);
  } finally {
    overwriteGetLocale(resolveLocale);
  }
});
