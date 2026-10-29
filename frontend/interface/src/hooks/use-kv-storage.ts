import {
  useCallback,
  useEffect,
  useMemo,
  useRef,
  useSyncExternalStore,
} from 'react';
import { commands } from '../ipc/bindings';
import {
  readKvStorageLocalCache,
  useKvStorageStore,
} from '../provider/kv-storage-provider';
import {
  canWriteKvStorageValue,
  decodeKvStorageValue,
  type KvStorageEntry,
} from '../provider/kv-storage-state';

export interface UseKvStorageOptions<T> {
  /**
   * Called with the raw parsed value from the cache, backend, or events.
   * Use this to transform old data shapes into the current shape.
   */
  migrate?: (value: unknown) => T;
}

type KvStorageView = {
  /** The shown JSON text, or `null` for the default. */
  raw: string | null;
  isLoading: boolean;
  readError: unknown | null;
};

function viewOf(
  entry: KvStorageEntry | undefined,
  cached: string | null,
): KvStorageView {
  if (!entry) return { raw: cached, isLoading: true, readError: null };
  return {
    raw:
      entry.draft?.value ?? (entry.value === undefined ? cached : entry.value),
    isLoading: entry.read.status !== 'done',
    readError: entry.read.error,
  };
}

/**
 * A `useState`-like hook backed by the Tauri/redb KV storage.
 *
 * - Every hook of a key in this window shares one value, kept by
 *   `KvStorageProvider`: the backend is read when the key is first used here,
 *   and its change events keep the value current, so mounting a used key
 *   costs no IPC.
 * - Until that read finishes, `isLoading` is true and the value comes from the
 *   localStorage cache, so the UI has a value on first render.
 * - Writing shows the new value at once in every hook of the key; writes of a
 *   key reach the backend in call order.
 * - The setter returns whether the write was confirmed. A failed value stays
 *   shown for a consumer to display and retry, until a newer value is
 *   committed.
 */
export function useKvStorage<T>(
  key: string,
  defaultValue: T,
  options?: UseKvStorageOptions<T>,
): readonly [
  T,
  (value: T | ((prev: T) => T)) => Promise<boolean>,
  {
    isLoading: boolean;
    readError: unknown | null;
    refresh: () => Promise<void>;
  },
] {
  const store = useKvStorageStore();

  const defaultValueRef = useRef(defaultValue);
  const migrateRef = useRef(options?.migrate);
  migrateRef.current = options?.migrate;

  const cached = useMemo(() => readKvStorageLocalCache(key), [key]);

  // Rendered in the same pass as the change, as `useState` would be, so a
  // handler that builds on the shown value never sees an older one.
  const viewRef = useRef<KvStorageView | null>(null);
  const subscribe = useCallback(
    (onChange: () => void) => store.subscribe(key, onChange),
    [store, key],
  );
  const view = useSyncExternalStore(subscribe, () => {
    const next = viewOf(store.peek(key), cached);
    const previous = viewRef.current;
    if (
      previous?.raw === next.raw &&
      previous.isLoading === next.isLoading &&
      previous.readError === next.readError
    ) {
      return previous;
    }
    viewRef.current = next;
    return next;
  });

  const decode = useCallback(
    (raw: string | null) =>
      decodeKvStorageValue(raw, defaultValueRef.current, migrateRef.current),
    [],
  );
  // Unchanged text keeps its value, so a confirmation of the shown value
  // spares every reader a render. Another key decodes with its own migration.
  const decoded = useMemo(() => decode(view.raw), [decode, key, view.raw]);
  const value = decoded.value;

  useEffect(() => {
    store.ensureRead(key);
  }, [store, key]);

  const setValue = useCallback(
    (next: T | ((prev: T) => T)) => {
      // Read the shared value, which may already hold another hook's write.
      const currentView = viewOf(store.peek(key), cached);
      const currentValue = decode(currentView.raw);
      const expectedRevision = () => {
        const entry = store.peek(key);
        if (!entry || entry.value === undefined) return null;
        if (!canWriteKvStorageValue(entry, decode(entry.value).error)) {
          return null;
        }
        return store.currentRevision();
      };
      if (expectedRevision() === null) {
        return Promise.resolve(false);
      }
      const resolved =
        typeof next === 'function'
          ? (next as (prev: T) => T)(currentValue.value)
          : next;
      return store.write(key, JSON.stringify(resolved), expectedRevision);
    },
    [store, key, cached, decode],
  );

  const refresh = useCallback(() => store.refresh(key), [store, key]);

  return [
    value,
    setValue,
    {
      isLoading: view.isLoading,
      readError: view.readError ?? decoded.error,
      refresh,
    },
  ] as const;
}

/**
 * Debug utilities for the backend KV store.
 * Not intended for production use — these bypass per-key subscriptions.
 */
export const kvStorageDebug = {
  /** Returns all stored key-value pairs with values deserialized from JSON. */
  async getAll(): Promise<Record<string, unknown>> {
    const result = await commands.getAllStorageItems();
    if (result.status === 'error') throw new Error(result.error);

    return Object.fromEntries(
      result.data.map(({ key, value }) => {
        try {
          return [key, JSON.parse(value)];
        } catch {
          return [key, value];
        }
      }),
    );
  },

  /** Removes every entry from the backend storage. */
  async clear(): Promise<void> {
    const result = await commands.clearStorage();
    if (result.status === 'error') throw new Error(result.error);
  },
};
