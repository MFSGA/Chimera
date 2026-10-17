/** Dashboard widget configuration contracts, defaults, and storage migration. */
export enum WidgetId {
  TrafficDown = 'traffic-down',
  TrafficUp = 'traffic-up',
  Connections = 'connections',
  Memory = 'memory',
  ProxyShortcuts = 'proxy-shortcuts',
  CoreShortcuts = 'core-shortcuts',
}

type TrafficConfig = {
  showChart: boolean;
  showTotal: boolean;
  unit: 'bytes' | 'bits';
};

type HistoryConfig = { showChart: boolean; samples: number };

export type WidgetConfigs = {
  [WidgetId.TrafficDown]: TrafficConfig & { type: WidgetId.TrafficDown };
  [WidgetId.TrafficUp]: TrafficConfig & { type: WidgetId.TrafficUp };
  [WidgetId.Connections]: HistoryConfig & { type: WidgetId.Connections };
  [WidgetId.Memory]: HistoryConfig & { type: WidgetId.Memory };
  [WidgetId.ProxyShortcuts]: {
    type: WidgetId.ProxyShortcuts;
    layout: 'equal' | 'flex';
    buttons: 'both' | 'system' | 'tun';
    order: 'system-first' | 'tun-first';
  };
  [WidgetId.CoreShortcuts]: {
    type: WidgetId.CoreShortcuts;
    showVersion: boolean;
  };
};

export type WidgetConfig = WidgetConfigs[WidgetId];

export const DEFAULT_WIDGET_CONFIGS: WidgetConfigs = {
  [WidgetId.TrafficDown]: {
    type: WidgetId.TrafficDown,
    showChart: true,
    showTotal: true,
    unit: 'bytes',
  },
  [WidgetId.TrafficUp]: {
    type: WidgetId.TrafficUp,
    showChart: true,
    showTotal: true,
    unit: 'bytes',
  },
  [WidgetId.Connections]: {
    type: WidgetId.Connections,
    showChart: true,
    samples: 32,
  },
  [WidgetId.Memory]: {
    type: WidgetId.Memory,
    showChart: true,
    samples: 32,
  },
  [WidgetId.ProxyShortcuts]: {
    type: WidgetId.ProxyShortcuts,
    layout: 'flex',
    buttons: 'both',
    order: 'system-first',
  },
  [WidgetId.CoreShortcuts]: {
    type: WidgetId.CoreShortcuts,
    showVersion: true,
  },
};

export type WidgetConfigStorage = {
  version: 1;
  byInstance: Record<string, WidgetConfig>;
};

export const EMPTY_WIDGET_CONFIG_STORAGE: WidgetConfigStorage = {
  version: 1,
  byInstance: {},
};

export const WIDGET_CONFIG_NUMBER_RANGES = {
  samples: [2, 32],
} as const;

const isRecord = (value: unknown): value is Record<string, unknown> =>
  typeof value === 'object' && value !== null && !Array.isArray(value);

/** Validate cached and persisted options without changing the layout schema. */
export function normalizeWidgetConfigStorage(
  raw: unknown,
): WidgetConfigStorage {
  if (!isRecord(raw) || raw.version !== 1 || !isRecord(raw.byInstance)) {
    return EMPTY_WIDGET_CONFIG_STORAGE;
  }

  const byInstance: Record<string, WidgetConfig> = {};
  for (const [id, value] of Object.entries(raw.byInstance)) {
    if (!isRecord(value)) continue;
    const defaults = Object.values(DEFAULT_WIDGET_CONFIGS).find(
      (config) => config.type === value.type,
    );
    if (!defaults) continue;

    const config = { ...defaults } as WidgetConfig;
    for (const [key, defaultValue] of Object.entries(defaults)) {
      const candidate = value[key];
      if (typeof candidate === 'boolean' && typeof defaultValue === 'boolean') {
        Object.assign(config, { [key]: candidate });
      } else if (key === 'samples' && typeof candidate === 'number') {
        const [min, max] = WIDGET_CONFIG_NUMBER_RANGES.samples;
        if (Number.isInteger(candidate)) {
          Object.assign(config, {
            samples: Math.min(max, Math.max(min, candidate)),
          });
        }
      } else if (
        (key === 'unit' && (candidate === 'bytes' || candidate === 'bits')) ||
        (key === 'layout' && (candidate === 'equal' || candidate === 'flex')) ||
        (key === 'buttons' &&
          (candidate === 'both' ||
            candidate === 'system' ||
            candidate === 'tun')) ||
        (key === 'order' &&
          (candidate === 'system-first' || candidate === 'tun-first'))
      ) {
        Object.assign(config, { [key]: candidate });
      }
    }

    Object.defineProperty(byInstance, id, {
      value: config,
      enumerable: true,
      configurable: true,
      writable: true,
    });
  }
  return { version: 1, byInstance };
}

export function getWidgetConfig<T extends WidgetId>(
  storage: WidgetConfigStorage,
  id: string,
  type: T,
): WidgetConfigs[T] {
  const saved = storage.byInstance[id];
  return (
    saved?.type === type ? saved : DEFAULT_WIDGET_CONFIGS[type]
  ) as WidgetConfigs[T];
}
