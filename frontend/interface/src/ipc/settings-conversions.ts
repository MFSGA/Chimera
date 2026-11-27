import type {
  NetworkStatisticWidgetConfig,
  StatisticWidgetVariant,
} from './bindings';

export type NetworkStatisticWidgetOption = 'disabled' | StatisticWidgetVariant;

export const toNetworkStatisticWidgetOption = (
  config: NetworkStatisticWidgetConfig,
): NetworkStatisticWidgetOption =>
  config.kind === 'enabled' ? config.value : 'disabled';

export const fromNetworkStatisticWidgetOption = (
  option: NetworkStatisticWidgetOption,
): NetworkStatisticWidgetConfig =>
  option === 'disabled'
    ? { kind: 'disabled' }
    : { kind: 'enabled', value: option };
