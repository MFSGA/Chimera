import assert from 'node:assert/strict';
import test from 'node:test';
import {
  DEFAULT_WIDGET_CONFIGS,
  EMPTY_WIDGET_CONFIG_STORAGE,
  WidgetId,
  withWidgetConfig,
} from '../src/components/widgets/widget-config.ts';
import { selectWidgetCalibrationSources } from '../src/components/widgets/widget-data-calibration-model.ts';

const item = (id: string, type: WidgetId) => ({ id, type });
const empty = { profiles: false, proxy: false, rule: false };

test('empty and unrelated dashboards do not poll profiles or providers', () => {
  assert.deepEqual(
    selectWidgetCalibrationSources([], EMPTY_WIDGET_CONFIG_STORAGE),
    empty,
  );
  assert.deepEqual(
    selectWidgetCalibrationSources(
      [
        item('memory', WidgetId.Memory),
        item('traffic', WidgetId.RecentTraffic),
      ],
      EMPTY_WIDGET_CONFIG_STORAGE,
    ),
    empty,
  );
});

test('Quota and Schedule enable only periodic profile calibration', () => {
  for (const type of [
    WidgetId.SubscriptionQuota,
    WidgetId.SubscriptionSchedule,
  ]) {
    assert.deepEqual(
      selectWidgetCalibrationSources(
        [item('subscription', type)],
        EMPTY_WIDGET_CONFIG_STORAGE,
      ),
      { profiles: true, proxy: false, rule: false },
    );
  }
});

test('provider reference filters activate only the required provider sources', () => {
  const proxyOnly = withWidgetConfig(
    EMPTY_WIDGET_CONFIG_STORAGE,
    'proxy-provider',
    { ...DEFAULT_WIDGET_CONFIGS[WidgetId.ProviderUpdates], kinds: 'proxy' },
  );
  const mixed = withWidgetConfig(proxyOnly, 'rule-provider', {
    ...DEFAULT_WIDGET_CONFIGS[WidgetId.ProviderUpdates],
    kinds: 'rule',
  });
  assert.deepEqual(
    selectWidgetCalibrationSources(
      [item('proxy-provider', WidgetId.ProviderUpdates)],
      mixed,
    ),
    { profiles: false, proxy: true, rule: false },
  );
  assert.deepEqual(
    selectWidgetCalibrationSources(
      [item('rule-provider', WidgetId.ProviderUpdates)],
      mixed,
    ),
    { profiles: false, proxy: false, rule: true },
  );
  assert.deepEqual(
    selectWidgetCalibrationSources(
      [
        item('proxy-provider', WidgetId.ProviderUpdates),
        item('rule-provider', WidgetId.ProviderUpdates),
      ],
      mixed,
    ),
    { profiles: false, proxy: true, rule: true },
  );
});

test('default provider and subscription widgets combine refresh sources', () => {
  assert.deepEqual(
    selectWidgetCalibrationSources(
      [
        item('sub', WidgetId.SubscriptionQuota),
        item('provider', WidgetId.ProviderUpdates),
      ],
      EMPTY_WIDGET_CONFIG_STORAGE,
    ),
    { profiles: true, proxy: true, rule: true },
  );
});
