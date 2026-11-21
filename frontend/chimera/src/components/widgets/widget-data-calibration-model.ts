import type { DashboardItem } from './consts';
import {
  getWidgetConfig,
  WidgetId,
  type WidgetConfigStorage,
} from './widget-config';

/** Match ref's data-source polling to the Widget instances actually shown. */
export function selectWidgetCalibrationSources(
  items: readonly Pick<DashboardItem, 'id' | 'type'>[],
  configs: WidgetConfigStorage,
) {
  const profiles = items.some(
    ({ type }) =>
      type === WidgetId.SubscriptionQuota ||
      type === WidgetId.SubscriptionSchedule,
  );
  const providers = items
    .filter(({ type }) => type === WidgetId.ProviderUpdates)
    .map(({ id }) => getWidgetConfig(configs, id, WidgetId.ProviderUpdates));

  return {
    profiles,
    proxy: providers.some(({ kinds }) => kinds !== 'rule'),
    rule: providers.some(({ kinds }) => kinds !== 'proxy'),
  };
}
