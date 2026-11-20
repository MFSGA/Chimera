import assert from 'node:assert/strict';
import test from 'node:test';
import {
  DEFAULT_WIDGET_CONFIGS,
  EMPTY_WIDGET_CONFIG_STORAGE,
  getWidgetConfig,
  normalizeWidgetConfigStorage,
  WidgetId,
} from '../src/components/widgets/widget-config.ts';

test('normalizes persisted widget settings and rejects invalid values', () => {
  const normalized = normalizeWidgetConfigStorage({
    version: 1,
    byInstance: {
      'traffic-down-0': {
        type: WidgetId.TrafficDown,
        showChart: false,
        showTotal: false,
        unit: 'bits',
        ignored: 'value',
      },
      'traffic-up-0': {
        type: WidgetId.TrafficUp,
        showChart: true,
        showTotal: true,
        unit: 'bits',
      },
      'connections-0': {
        type: WidgetId.Connections,
        showChart: true,
        samples: 100,
      },
      'memory-0': {
        type: WidgetId.Memory,
        showChart: false,
        samples: 1.5,
      },
      'proxy-shortcuts-0': {
        type: WidgetId.ProxyShortcuts,
        layout: 'unknown',
        buttons: 'system',
        order: 'tun-first',
      },
      'core-shortcuts-0': {
        type: WidgetId.CoreShortcuts,
        showVersion: false,
      },
      'unknown-0': { type: 'unknown-widget' },
    },
  });

  assert.deepEqual(normalized.byInstance['traffic-down-0'], {
    type: WidgetId.TrafficDown,
    showChart: false,
    showTotal: false,
    unit: 'bits',
  });
  assert.deepEqual(normalized.byInstance['traffic-up-0'], {
    ...DEFAULT_WIDGET_CONFIGS[WidgetId.TrafficUp],
    unit: 'bits',
  });
  assert.equal(
    normalized.byInstance['connections-0']?.type,
    WidgetId.Connections,
  );
  assert.equal(
    normalized.byInstance['connections-0']?.type === WidgetId.Connections &&
      normalized.byInstance['connections-0'].samples,
    32,
  );
  assert.equal(
    normalized.byInstance['memory-0']?.type === WidgetId.Memory &&
      normalized.byInstance['memory-0'].samples,
    32,
  );
  assert.deepEqual(normalized.byInstance['proxy-shortcuts-0'], {
    type: WidgetId.ProxyShortcuts,
    layout: 'flex',
    buttons: 'system',
    order: 'tun-first',
  });
  assert.deepEqual(normalized.byInstance['core-shortcuts-0'], {
    type: WidgetId.CoreShortcuts,
    showVersion: false,
  });
  assert.equal('unknown-0' in normalized.byInstance, false);
});

test('Network Info address visibility persists and rejects non-booleans', () => {
  const saved = normalizeWidgetConfigStorage({
    version: 1,
    byInstance: {
      hidden: { type: WidgetId.NetworkInfo, hideAddresses: true },
      invalid: { type: WidgetId.NetworkInfo, hideAddresses: 'yes' },
    },
  });
  assert.deepEqual(getWidgetConfig(saved, 'hidden', WidgetId.NetworkInfo), {
    type: WidgetId.NetworkInfo,
    hideAddresses: true,
  });
  assert.deepEqual(
    getWidgetConfig(saved, 'invalid', WidgetId.NetworkInfo),
    DEFAULT_WIDGET_CONFIGS[WidgetId.NetworkInfo],
  );
});

test('Provider Updates keeps pinned resources and constrains invalid saved options', () => {
  const saved = normalizeWidgetConfigStorage({
    version: 1,
    byInstance: {
      selected: {
        type: WidgetId.ProviderUpdates,
        kinds: 'rule',
        maxItems: 50,
        resources: [
          { kind: 'proxy', name: 'Alpha' },
          { kind: 'proxy', name: 'Alpha' },
          { kind: 'rule', name: 'Rule One' },
          { kind: 'random', name: 'invalid' },
          { kind: 'proxy', name: '' },
        ],
      },
      broken: {
        type: WidgetId.ProviderUpdates,
        kinds: 'untrusted',
        maxItems: 0,
        resources: 'broken',
      },
      low: {
        type: WidgetId.ProviderUpdates,
        maxItems: 1.5,
      },
    },
  });
  assert.deepEqual(
    getWidgetConfig(saved, 'selected', WidgetId.ProviderUpdates),
    {
      type: WidgetId.ProviderUpdates,
      kinds: 'rule',
      maxItems: 20,
      resources: [
        { kind: 'proxy', name: 'Alpha' },
        { kind: 'rule', name: 'Rule One' },
      ],
    },
  );
  assert.deepEqual(getWidgetConfig(saved, 'broken', WidgetId.ProviderUpdates), {
    type: WidgetId.ProviderUpdates,
    kinds: 'both',
    maxItems: 1,
    resources: [],
  });
  assert.deepEqual(
    getWidgetConfig(saved, 'low', WidgetId.ProviderUpdates),
    DEFAULT_WIDGET_CONFIGS[WidgetId.ProviderUpdates],
  );
});

test('normalizes Subscription Quota controls without accepting invalid thresholds', () => {
  const storage = normalizeWidgetConfigStorage({
    version: 1,
    byInstance: {
      custom: {
        type: WidgetId.SubscriptionQuota,
        target: { kind: 'fixed', profileUid: 'remote-a' },
        showExpiry: false,
        showProgress: false,
        waveStyle: 'outline',
        animateWave: false,
        expiryWarningDays: 500,
        quotaWarningPercent: 0,
      },
      malformed: {
        type: WidgetId.SubscriptionQuota,
        target: { kind: 'fixed', profileUid: '' },
        waveStyle: 'invalid',
        expiryWarningDays: 2.5,
        quotaWarningPercent: NaN,
      },
    },
  });
  assert.deepEqual(
    getWidgetConfig(storage, 'custom', WidgetId.SubscriptionQuota),
    {
      type: WidgetId.SubscriptionQuota,
      target: { kind: 'fixed', profileUid: 'remote-a' },
      showExpiry: false,
      showProgress: false,
      waveStyle: 'outline',
      animateWave: false,
      expiryWarningDays: 365,
      quotaWarningPercent: 1,
    },
  );
  assert.deepEqual(
    getWidgetConfig(storage, 'malformed', WidgetId.SubscriptionQuota),
    DEFAULT_WIDGET_CONFIGS[WidgetId.SubscriptionQuota],
  );
});

test('preserves only valid Subscription Schedule targets and history settings', () => {
  const stored = normalizeWidgetConfigStorage({
    version: 1,
    byInstance: {
      fixed: {
        type: WidgetId.SubscriptionSchedule,
        target: { kind: 'fixed', profileUid: 'remote-a' },
        showRecentRuns: false,
      },
      empty: {
        type: WidgetId.SubscriptionSchedule,
        target: { kind: 'fixed', profileUid: '' },
      },
      malformed: {
        type: WidgetId.SubscriptionSchedule,
        target: { kind: 'unknown', profileUid: 'remote-a' },
      },
      follow: {
        type: WidgetId.SubscriptionSchedule,
        target: { kind: 'current', profileUid: 'ignored' },
      },
    },
  });
  assert.deepEqual(
    getWidgetConfig(stored, 'fixed', WidgetId.SubscriptionSchedule),
    {
      type: WidgetId.SubscriptionSchedule,
      target: { kind: 'fixed', profileUid: 'remote-a' },
      showRecentRuns: false,
    },
  );
  for (const id of ['empty', 'malformed']) {
    assert.deepEqual(
      getWidgetConfig(stored, id, WidgetId.SubscriptionSchedule).target,
      { kind: 'current' },
    );
  }
  assert.deepEqual(
    getWidgetConfig(stored, 'follow', WidgetId.SubscriptionSchedule).target,
    { kind: 'current' },
  );
});

test('normalizes Proxy Mode layout without changing Proxy Shortcuts semantics', () => {
  const stored = normalizeWidgetConfigStorage({
    version: 1,
    byInstance: {
      'proxy-mode-0': { type: WidgetId.ProxyMode, layout: 'focus' },
      'proxy-mode-1': { type: WidgetId.ProxyMode, layout: 'equal' },
      'proxy-shortcuts-0': {
        type: WidgetId.ProxyShortcuts,
        layout: 'equal',
        buttons: 'both',
        order: 'system-first',
      },
    },
  });

  assert.equal(
    getWidgetConfig(stored, 'proxy-mode-0', WidgetId.ProxyMode).layout,
    'focus',
  );
  assert.equal(
    getWidgetConfig(stored, 'proxy-mode-1', WidgetId.ProxyMode).layout,
    'flex',
  );
  assert.equal(
    getWidgetConfig(stored, 'proxy-shortcuts-0', WidgetId.ProxyShortcuts)
      .layout,
    'equal',
  );
});

test('rejects unknown versions and defaults malformed storage and mismatches', () => {
  for (const version of [2, '2']) {
    assert.throws(
      () => normalizeWidgetConfigStorage({ version, byInstance: {} }),
      new RegExp(`Unsupported dashboard widget config version: ${version}`),
    );
  }
  assert.equal(
    getWidgetConfig(
      { version: 1, byInstance: {} },
      'traffic-down-0',
      WidgetId.TrafficDown,
    ),
    DEFAULT_WIDGET_CONFIGS[WidgetId.TrafficDown],
  );
  assert.equal(
    getWidgetConfig(
      {
        version: 1,
        byInstance: {
          'traffic-down-0': DEFAULT_WIDGET_CONFIGS[WidgetId.TrafficUp],
        },
      },
      'traffic-down-0',
      WidgetId.TrafficDown,
    ),
    DEFAULT_WIDGET_CONFIGS[WidgetId.TrafficDown],
  );
});

test('keeps prototype-like instance IDs as ordinary own entries', () => {
  const raw = JSON.parse(
    '{"version":1,"byInstance":{"__proto__":{"type":"traffic-down","unit":"bits"}}}',
  );
  const normalized = normalizeWidgetConfigStorage(raw);

  assert.equal(Object.getPrototypeOf(normalized.byInstance), Object.prototype);
  assert.equal(Object.hasOwn(normalized.byInstance, '__proto__'), true);
  assert.deepEqual(normalized.byInstance['__proto__'], {
    ...DEFAULT_WIDGET_CONFIGS[WidgetId.TrafficDown],
    unit: 'bits',
  });
});
