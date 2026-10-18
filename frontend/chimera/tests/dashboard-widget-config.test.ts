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
