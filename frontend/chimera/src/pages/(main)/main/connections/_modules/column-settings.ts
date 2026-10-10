import { getStorageItem } from '@chimera/interface';
import {
  functionalUpdate,
  type ColumnOrderState,
  type ColumnSizingState,
  type ColumnVisibilityState,
  type Updater,
} from '@tanstack/react-table';
import { useEffect } from 'react';
import { useLocalStorage } from '@/hooks/use-local-storage';

type ColumnSettings = {
  order: ColumnOrderState;
  visibility: ColumnVisibilityState;
};

const DEFAULT_COLUMN_SETTINGS: ColumnSettings = { order: [], visibility: {} };

// Saved settings outlive the columns they name: ids that are gone are dropped,
// and columns added since follow the saved ones in their default order.
export function resolveColumnOrder(
  saved: readonly string[],
  ids: readonly string[],
): string[] {
  const known = new Set(ids);
  const kept = [...new Set(saved)].filter((id) => known.has(id));
  const placed = new Set(kept);

  return [...kept, ...ids.filter((id) => !placed.has(id))];
}

// The former Legacy table persisted these keys in the app's KV store.
// Convert at the edge once, without teaching the shared table old ids.
const LEGACY_COLUMN_IDS: Record<string, string> = {
  host: 'Host',
  process: 'Process',
  downloaded: 'Downloaded',
  uploaded: 'Uploaded',
  dl_speed: 'DL Speed',
  ul_speed: 'UL Speed',
  chains: 'Chains',
  rule: 'Rule',
  time: 'Time',
  source: 'Source',
  destination_ip: 'Destination IP',
  type: 'Type',
};

export function migrateLegacyColumnSettings(
  value: unknown,
  ids: readonly string[],
): ColumnSettings | null {
  if (!Array.isArray(value)) {
    return null;
  }

  const known = new Set(ids);
  const order: string[] = [];
  const visibility: ColumnVisibilityState = {};

  for (const item of value) {
    if (
      !Array.isArray(item) ||
      typeof item[0] !== 'string' ||
      typeof item[1] !== 'boolean'
    ) {
      continue;
    }

    const id = LEGACY_COLUMN_IDS[item[0]];
    if (!id || !known.has(id) || order.includes(id)) {
      continue;
    }
    order.push(id);
    visibility[id] = item[1];
  }

  if (order.length === 0) {
    return null;
  }
  if (ids.every((id) => visibility[id] === false)) {
    visibility[ids[0]] = true;
  }
  return { order: resolveColumnOrder(order, ids), visibility };
}

export function migrateLegacyColumnSizing(
  sizing: ColumnSizingState,
): ColumnSizingState {
  const oldEntries = Object.entries(sizing).filter(
    ([id]) => LEGACY_COLUMN_IDS[id] !== undefined,
  );
  if (oldEntries.length === 0) {
    return sizing;
  }

  const next: ColumnSizingState = { ...sizing };
  for (const [oldId, width] of oldEntries) {
    const newId = LEGACY_COLUMN_IDS[oldId];
    // Widths adjusted after the migration must always win.
    next[newId] ??= width;
    delete next[oldId];
  }
  return next;
}

export function useColumnSettings(storageKey: string, ids: readonly string[]) {
  const [settings, setSettings] = useLocalStorage<ColumnSettings>(
    storageKey,
    DEFAULT_COLUMN_SETTINGS,
  );

  useEffect(() => {
    // Never replace settings a user has already saved under the ref key.
    if (localStorage.getItem(storageKey) !== null) {
      return;
    }
    let cancelled = false;

    getStorageItem('connections_table_columns')
      .then((saved) => {
        if (
          cancelled ||
          saved === null ||
          localStorage.getItem(storageKey) !== null
        ) {
          return;
        }
        let parsed: unknown;
        try {
          parsed = JSON.parse(saved);
        } catch {
          return;
        }
        const migrated = migrateLegacyColumnSettings(parsed, ids);
        if (migrated) {
          setSettings(migrated);
        }
      })
      .catch((error: unknown) => {
        console.warn(
          'Legacy Connections column settings could not be read:',
          error,
        );
      });

    return () => {
      cancelled = true;
    };
  }, [storageKey, ids, setSettings]);

  // Storage is editable by hand, so a missing field falls back to its default.
  const { order: savedOrder = [], visibility = {} } = settings ?? {};
  const order = resolveColumnOrder(savedOrder, ids);

  return {
    order,
    visibility,
    setOrder: (updater: Updater<ColumnOrderState>) =>
      setSettings({ order: functionalUpdate(updater, order), visibility }),
    setVisibility: (updater: Updater<ColumnVisibilityState>) =>
      setSettings({ order, visibility: functionalUpdate(updater, visibility) }),
  };
}
