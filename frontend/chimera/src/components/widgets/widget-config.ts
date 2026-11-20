/** Dashboard widget configuration contracts, defaults, and storage migration. */
export enum WidgetId {
  TrafficDown = 'traffic-down',
  TrafficUp = 'traffic-up',
  Connections = 'connections',
  Memory = 'memory',
  ProxyShortcuts = 'proxy-shortcuts',
  CoreShortcuts = 'core-shortcuts',
  ProxyMode = 'proxy-mode',
  SubscriptionSchedule = 'subscription-schedule',
  SubscriptionQuota = 'subscription-quota',
  ProviderUpdates = 'provider-updates',
  NetworkInfo = 'network-info',
  ActiveConnections = 'active-connections',
}

type TrafficConfig = {
  showChart: boolean;
  showTotal: boolean;
  unit: 'bytes' | 'bits';
};

type HistoryConfig = { showChart: boolean; samples: number };

/** Reference targets for upcoming subscription/report dashboard widgets. */
export type SubscriptionTarget =
  { kind: 'current' } | { kind: 'fixed'; profileUid: string };

export type SubscriptionQuotaWaveStyle = 'filled' | 'outline';

export type ProviderReference = { kind: 'proxy' | 'rule'; name: string };

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
  [WidgetId.ProxyMode]: {
    type: WidgetId.ProxyMode;
    layout: 'focus' | 'flex';
  };
  [WidgetId.SubscriptionSchedule]: {
    type: WidgetId.SubscriptionSchedule;
    target: SubscriptionTarget;
    showRecentRuns: boolean;
  };
  [WidgetId.SubscriptionQuota]: {
    type: WidgetId.SubscriptionQuota;
    target: SubscriptionTarget;
    showExpiry: boolean;
    showProgress: boolean;
    waveStyle: SubscriptionQuotaWaveStyle;
    animateWave: boolean;
    expiryWarningDays: number;
    quotaWarningPercent: number;
  };
  [WidgetId.ProviderUpdates]: {
    type: WidgetId.ProviderUpdates;
    kinds: 'both' | 'proxy' | 'rule';
    resources: ProviderReference[];
    maxItems: number;
  };
  [WidgetId.NetworkInfo]: {
    type: WidgetId.NetworkInfo;
    hideAddresses: boolean;
  };
  [WidgetId.ActiveConnections]: {
    type: WidgetId.ActiveConnections;
    sort: 'download' | 'upload' | 'total';
    topN: number;
    showProcess: boolean;
    hideTargets: boolean;
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
  [WidgetId.ProxyMode]: {
    type: WidgetId.ProxyMode,
    layout: 'flex',
  },
  [WidgetId.SubscriptionSchedule]: {
    type: WidgetId.SubscriptionSchedule,
    target: { kind: 'current' },
    showRecentRuns: true,
  },
  [WidgetId.SubscriptionQuota]: {
    type: WidgetId.SubscriptionQuota,
    target: { kind: 'current' },
    showExpiry: true,
    showProgress: true,
    waveStyle: 'filled',
    animateWave: true,
    expiryWarningDays: 7,
    quotaWarningPercent: 20,
  },
  [WidgetId.ProviderUpdates]: {
    type: WidgetId.ProviderUpdates,
    kinds: 'both',
    resources: [],
    maxItems: 3,
  },
  [WidgetId.NetworkInfo]: {
    type: WidgetId.NetworkInfo,
    hideAddresses: false,
  },
  [WidgetId.ActiveConnections]: {
    type: WidgetId.ActiveConnections,
    sort: 'download',
    topN: 3,
    showProcess: true,
    hideTargets: false,
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
  expiryWarningDays: [1, 365],
  quotaWarningPercent: [1, 100],
  maxItems: [1, 20],
  topN: [1, 20],
} as const;

const isRecord = (value: unknown): value is Record<string, unknown> =>
  typeof value === 'object' && value !== null && !Array.isArray(value);

/** Validate cached and persisted options without changing the layout schema. */
export function normalizeWidgetConfigStorage(
  raw: unknown,
): WidgetConfigStorage {
  if (isRecord(raw) && Object.hasOwn(raw, 'version') && raw.version !== 1) {
    throw new Error(
      `Unsupported dashboard widget config version: ${String(raw.version)}`,
    );
  }

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
      if (
        key === 'target' &&
        isRecord(candidate) &&
        (candidate.kind === 'current' ||
          (candidate.kind === 'fixed' &&
            typeof candidate.profileUid === 'string' &&
            candidate.profileUid.trim().length > 0))
      ) {
        Object.assign(config, {
          target:
            candidate.kind === 'current'
              ? { kind: 'current' }
              : { kind: 'fixed', profileUid: candidate.profileUid },
        });
      } else if (key === 'resources' && Array.isArray(candidate)) {
        const resources: ProviderReference[] = [];
        const seen = new Set<string>();
        for (const entry of candidate) {
          if (!isRecord(entry)) continue;
          if (entry.kind !== 'proxy' && entry.kind !== 'rule') continue;
          if (typeof entry.name !== 'string' || !entry.name.trim()) continue;
          const identity = JSON.stringify([entry.kind, entry.name]);
          if (seen.has(identity)) continue;
          seen.add(identity);
          resources.push({ kind: entry.kind, name: entry.name });
        }
        Object.assign(config, { resources });
      } else if (
        typeof candidate === 'boolean' &&
        typeof defaultValue === 'boolean'
      ) {
        Object.assign(config, { [key]: candidate });
      } else if (
        (key === 'samples' ||
          key === 'expiryWarningDays' ||
          key === 'quotaWarningPercent' ||
          key === 'maxItems' ||
          key === 'topN') &&
        typeof candidate === 'number' &&
        Number.isInteger(candidate)
      ) {
        const [min, max] = WIDGET_CONFIG_NUMBER_RANGES[key];
        Object.assign(config, {
          [key]: Math.min(max, Math.max(min, candidate)),
        });
      } else if (
        (key === 'unit' && (candidate === 'bytes' || candidate === 'bits')) ||
        (key === 'waveStyle' &&
          (candidate === 'filled' || candidate === 'outline')) ||
        (key === 'sort' &&
          (candidate === 'download' ||
            candidate === 'upload' ||
            candidate === 'total')) ||
        (key === 'kinds' &&
          (candidate === 'both' ||
            candidate === 'proxy' ||
            candidate === 'rule')) ||
        (key === 'layout' &&
          ((config.type === WidgetId.ProxyMode &&
            (candidate === 'focus' || candidate === 'flex')) ||
            (config.type === WidgetId.ProxyShortcuts &&
              (candidate === 'equal' || candidate === 'flex')))) ||
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
