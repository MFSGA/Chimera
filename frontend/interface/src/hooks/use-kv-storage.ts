import { useCallback, useEffect, useRef, useState } from 'react';
import { commands, events } from '../ipc/bindings';
import { KvStorageWriteEchoes } from './kv-storage-write-echoes';

const LOCAL_CACHE_PREFIX = 'nyanpasu-kv-:';
/** Mirrors the `WEB_STORAGE_KEY_PREFIX` constant on the backend. */
const WEB_KEY_PREFIX = 'web:';

function getLocalCache<T>(
  key: string,
  defaultValue: T,
  migrate?: (value: unknown) => T,
): T {
  try {
    const raw = localStorage.getItem(LOCAL_CACHE_PREFIX + btoa(key));
    if (raw === null) return defaultValue;
    const parsed: unknown = JSON.parse(raw);
    return migrate ? migrate(parsed) : (parsed as T);
  } catch {
    return defaultValue;
  }
}

function setLocalCache<T>(key: string, value: T): void {
  try {
    localStorage.setItem(LOCAL_CACHE_PREFIX + btoa(key), JSON.stringify(value));
  } catch {
    // ignore quota / security errors
  }
}

function removeLocalCache(key: string): void {
  try {
    localStorage.removeItem(LOCAL_CACHE_PREFIX + btoa(key));
  } catch {
    // ignore storage access errors
  }
}

export interface UseKvStorageOptions<T> {
  /**
   * Called with the raw parsed value from the cache, backend, or events.
   * Use this to transform old data shapes into the current shape.
   */
  migrate?: (value: unknown) => T;
}

/**
 * A `useState`-like hook backed by the Tauri/redb KV storage.
 *
 * - Reads the local cache immediately and then fetches the authoritative value.
 * - Keeps optimistic writes from being replaced by an older read or event.
 * - Serializes writes from this hook instance and reports whether storage accepted them.
 * - Listens for storage events so open windows stay in sync.
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
  const [value, setValueState] = useState<T>(() =>
    getLocalCache(key, defaultValue, options?.migrate),
  );
  const [isLoading, setIsLoading] = useState(true);
  const [readError, setReadError] = useState<unknown | null>(null);

  const defaultValueRef = useRef(defaultValue);
  defaultValueRef.current = defaultValue;
  const valueRef = useRef(value);
  valueRef.current = value;
  const migrateRef = useRef(options?.migrate);
  migrateRef.current = options?.migrate;
  // Keep old-key echoes until the global event stream confirms those writes.
  const pendingWritesRef = useRef(new KvStorageWriteEchoes(key));
  const revisionRef = useRef(new Map<string, number>());
  const activeWritesRef = useRef(new Map<string, number>());
  const writeQueueRef = useRef<Promise<unknown>>(Promise.resolve());
  const cachedKeyRef = useRef(key);
  const refreshRef = useRef<() => Promise<void>>(async () => {});
  const refreshStorage = useCallback(() => refreshRef.current(), []);

  const applyMigrate = useCallback((raw: unknown): T => {
    return migrateRef.current ? migrateRef.current(raw) : (raw as T);
  }, []);

  useEffect(() => {
    if (cachedKeyRef.current !== key) {
      cachedKeyRef.current = key;
      pendingWritesRef.current.setActiveKey(key);
      const cached = getLocalCache(
        key,
        defaultValueRef.current,
        migrateRef.current,
      );
      valueRef.current = cached;
      setValueState(cached);
      setIsLoading(true);
    }

    let disposed = false;
    const refresh = async () => {
      const revision = revisionRef.current.get(key) ?? 0;
      const readingDuringWrite = (activeWritesRef.current.get(key) ?? 0) > 0;
      setIsLoading(true);
      try {
        const result = await commands.getStorageItem(key);
        if (disposed) return;
        if (result.status === 'error') throw new Error(result.error);
        if (
          readingDuringWrite ||
          (activeWritesRef.current.get(key) ?? 0) > 0 ||
          revision !== (revisionRef.current.get(key) ?? 0)
        ) {
          return;
        }

        if (result.data === null) {
          valueRef.current = defaultValueRef.current;
          setValueState(defaultValueRef.current);
          removeLocalCache(key);
        } else {
          const migrated = applyMigrate(JSON.parse(result.data));
          if (JSON.stringify(migrated) !== JSON.stringify(valueRef.current)) {
            valueRef.current = migrated;
            setValueState(migrated);
            setLocalCache(key, migrated);
          }
        }
        setReadError(null);
      } catch (error) {
        if (disposed) return;
        setReadError(error);
        console.error('[useKvStorage] getStorageItem failed:', error);
      } finally {
        if (!disposed) setIsLoading(false);
      }
    };

    refreshRef.current = refresh;
    void refresh();
    return () => {
      disposed = true;
    };
  }, [key, applyMigrate]);

  useEffect(() => {
    const unlistenPromise = events.storageValueChangedEvent.listen((event) => {
      if (!event.payload.key.startsWith(WEB_KEY_PREFIX)) return;
      const eventKey = event.payload.key.slice(WEB_KEY_PREFIX.length);

      // Consume echoes for inactive keys too; the write may have completed
      // while this hook was displaying a different key.
      if (
        event.payload.value !== null &&
        pendingWritesRef.current.consume(eventKey, event.payload.value)
      ) {
        return;
      }
      if (!pendingWritesRef.current.isActiveKey(key) || eventKey !== key)
        return;

      if (event.payload.value === null) {
        revisionRef.current.set(key, (revisionRef.current.get(key) ?? 0) + 1);
        valueRef.current = defaultValueRef.current;
        setValueState(defaultValueRef.current);
        setReadError(null);
        removeLocalCache(key);
        return;
      }

      try {
        const firstParsed = JSON.parse(event.payload.value);
        const parsed =
          typeof firstParsed === 'string'
            ? JSON.parse(firstParsed)
            : firstParsed;
        const migrated = applyMigrate(parsed);
        revisionRef.current.set(key, (revisionRef.current.get(key) ?? 0) + 1);
        valueRef.current = migrated;
        setValueState(migrated);
        setReadError(null);
        setLocalCache(key, migrated);
      } catch {
        // ignore invalid JSON from event
      }
    });

    return () => {
      unlistenPromise.then((fn) => fn());
    };
  }, [key, applyMigrate]);

  const setValue = useCallback(
    async (newValue: T | ((prev: T) => T)) => {
      const resolved =
        typeof newValue === 'function'
          ? (newValue as (prev: T) => T)(valueRef.current)
          : newValue;
      const serialized = JSON.stringify(resolved);
      revisionRef.current.set(key, (revisionRef.current.get(key) ?? 0) + 1);
      activeWritesRef.current.set(
        key,
        (activeWritesRef.current.get(key) ?? 0) + 1,
      );
      const serializedEcho = JSON.stringify(serialized);
      pendingWritesRef.current.add(key, serializedEcho);
      valueRef.current = resolved;
      setValueState(resolved);
      setLocalCache(key, resolved);

      const previousWrite = writeQueueRef.current;
      const write = (async () => {
        await previousWrite;
        return commands.setStorageItem(key, serialized);
      })();
      writeQueueRef.current = write.catch(() => {});

      try {
        const result = await write;
        if (result.status === 'error') throw new Error(result.error);
        return true;
      } catch (error) {
        pendingWritesRef.current.remove(key, serializedEcho);
        console.error('[useKvStorage] setStorageItem failed:', error);
        return false;
      } finally {
        const activeWritesForKey = (activeWritesRef.current.get(key) ?? 1) - 1;
        if (activeWritesForKey === 0) activeWritesRef.current.delete(key);
        else activeWritesRef.current.set(key, activeWritesForKey);
      }
    },
    [key],
  );

  return [
    value,
    setValue,
    { isLoading, readError, refresh: refreshStorage },
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

    if (result.status === 'error') {
      throw new Error(result.error);
    }

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

    if (result.status === 'error') {
      throw new Error(result.error);
    }
  },
};
