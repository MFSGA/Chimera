import {
  hashKey,
  skipToken,
  useQueryClient,
  type QueryClient,
} from '@tanstack/react-query';
import {
  createContext,
  useContext,
  useEffect,
  useMemo,
  type PropsWithChildren,
} from 'react';
import {
  commands,
  events,
  type StorageResyncEvent,
  type StorageValueChangedEvent,
} from '../ipc/bindings';
import {
  EMPTY_KV_STORAGE_ENTRY,
  isKvStorageRevisionConflict,
  kvStorageEventSnapshot,
  kvStorageQueryKey,
  mergeKvStorageSnapshot,
  type KvStorageEntry,
} from './kv-storage-state';

const LOCAL_CACHE_PREFIX = 'nyanpasu-kv-:';

export type KvStorageApi = {
  getStorageItem: typeof commands.getStorageItem;
  setStorageItem: typeof commands.setStorageItem;
  listenStorageValueChanged: (
    handler: (event: { payload: StorageValueChangedEvent }) => void,
  ) => Promise<() => void>;
  listenStorageResync: (
    handler: (event: { payload: StorageResyncEvent }) => void,
  ) => Promise<() => void>;
};

const tauriKvStorageApi: KvStorageApi = {
  getStorageItem: commands.getStorageItem,
  setStorageItem: commands.setStorageItem,
  listenStorageValueChanged: (handler) =>
    events.storageValueChangedEvent.listen(handler),
  listenStorageResync: (handler) => events.storageResyncEvent.listen(handler),
};

/** The last value this browser profile showed for `key`, as JSON text. */
export function readKvStorageLocalCache(key: string): string | null {
  try {
    return localStorage.getItem(LOCAL_CACHE_PREFIX + btoa(key));
  } catch {
    return null;
  }
}

function mirrorKvStorageLocalCache(key: string, entry: KvStorageEntry) {
  const value = entry.draft?.value ?? entry.value;
  if (value === undefined) return;
  try {
    if (value === null) localStorage.removeItem(LOCAL_CACHE_PREFIX + btoa(key));
    else localStorage.setItem(LOCAL_CACHE_PREFIX + btoa(key), value);
  } catch {
    // ignore quota / security errors
  }
}

export type KvStorageStore = ReturnType<typeof createKvStorageStore>;

/**
 * Owns the storage values of one query client: each key is read once, kept
 * current by the backend's events, and written through one queue per key so
 * every hook of a key shows the same value.
 */
export function createKvStorageStore(
  queryClient: QueryClient,
  api: KvStorageApi = tauriKvStorageApi,
  listenerRetryInitialDelayMs = 250,
) {
  let markReady = () => {};
  // The first reads wait for the listeners, so nothing committed after a read
  // goes unheard.
  const ready = new Promise<void>((resolve) => {
    markReady = resolve;
  });
  const reads = new Map<string, Promise<void>>();
  const writeTails = new Map<string, Promise<boolean>>();
  const writeEpochs = new Map<string, number>();
  const activeWrites = new Map<
    string,
    { value: string; expectedRevision: number | null }
  >();
  let draftSequence = 0;
  let observedRevision = 0;

  const peek = (key: string) =>
    queryClient.getQueryData<KvStorageEntry>(kvStorageQueryKey(key));

  const update = (
    key: string,
    change: (entry: KvStorageEntry) => KvStorageEntry,
  ) => {
    const entry = queryClient.setQueryData<KvStorageEntry>(
      kvStorageQueryKey(key),
      (entry) => change(entry ?? EMPTY_KV_STORAGE_ENTRY),
    );
    if (entry) mirrorKvStorageLocalCache(key, entry);
  };

  const isActiveWriteEcho = (
    key: string,
    snapshot: { value: string | null; revision: number },
  ) => {
    const active = activeWrites.get(key);
    if (
      active &&
      active.value === snapshot.value &&
      snapshot.revision > (active.expectedRevision ?? -1)
    ) {
      return true;
    }
    return false;
  };

  const applySnapshot = (
    key: string,
    snapshot: { value: string | null; revision: number },
  ) => {
    const current = peek(key) ?? EMPTY_KV_STORAGE_ENTRY;
    const replacesValue =
      current.revision !== null &&
      snapshot.revision > current.revision &&
      snapshot.value !== current.value;
    const localEcho = isActiveWriteEcho(key, snapshot);
    if (replacesValue && !localEcho) {
      writeEpochs.set(key, (writeEpochs.get(key) ?? 0) + 1);
      update(key, (entry) => ({
        ...mergeKvStorageSnapshot(entry, snapshot),
        draft: null,
      }));
      return;
    }
    update(key, (entry) => mergeKvStorageSnapshot(entry, snapshot));
  };

  // `isCurrent` turns false once a newer read of the key has started, which
  // then settles the key instead.
  const settleRead = async (key: string, isCurrent: () => boolean) => {
    await ready;
    try {
      const result = await api.getStorageItem(key);
      if (!isCurrent()) return;
      if (result.status === 'error') throw result.error;
      observedRevision = Math.max(observedRevision, result.data.revision);
      const entry = peek(key) ?? EMPTY_KV_STORAGE_ENTRY;
      const merged = mergeKvStorageSnapshot(entry, {
        value: result.data.value,
        revision: result.data.revision,
      });
      // Unreadable stored JSON is a failed read, not a default to save over.
      if (merged !== entry && merged.value != null) JSON.parse(merged.value);
      applySnapshot(key, {
        value: result.data.value,
        revision: result.data.revision,
      });
      update(key, (current) => ({
        ...current,
        read: { status: 'done', error: null },
      }));
    } catch (error) {
      if (!isCurrent()) return;
      console.error('[useKvStorage] read failed:', error);
      update(key, (entry) => ({ ...entry, read: { status: 'done', error } }));
    }
  };

  const read = (key: string) => {
    update(key, (entry) => ({
      ...entry,
      read: { status: 'reading', error: entry.read.error },
    }));
    const isCurrent = () => reads.get(key) === promise;
    const promise: Promise<void> = settleRead(key, isCurrent).finally(() => {
      if (isCurrent()) reads.delete(key);
    });
    reads.set(key, promise);
    return promise;
  };

  // Values missed in a gap may have been committed after a read in flight
  // was answered, so a resync starts fresh reads.
  const resync = () => {
    for (const query of queryClient
      .getQueryCache()
      .findAll({ queryKey: ['kvStorage'] })) {
      read(query.queryKey[1] as string);
    }
  };

  return {
    peek,
    currentRevision: () => observedRevision,

    /** Calls `onChange`, in the same task, whenever the entry of `key` changes. */
    subscribe: (key: string, onChange: () => void) => {
      const queryHash = hashKey(kvStorageQueryKey(key));
      return queryClient.getQueryCache().subscribe((event) => {
        if (event.query.queryHash === queryHash) onChange();
      });
    },

    /** Reads `key` unless this client has read it or is reading it. */
    ensureRead: (key: string) => {
      if ((peek(key)?.read.status ?? 'idle') === 'idle') read(key);
    },

    refresh: (key: string) => reads.get(key) ?? read(key),

    /**
     * Shows `value` at once and stores it after the earlier writes of `key`.
     * An optional revision callback is checked when a queued write reaches
     * the backend, so stale drafts cannot overwrite a newer committed value.
     * A revision conflict invalidates the drafts already queued for this key.
     * A backend failure keeps its draft shown; a rejected guard removes it so
     * the committed value becomes visible again.
     */
    write: (
      key: string,
      value: string,
      expectedRevision?: () => number | null,
    ): Promise<boolean> => {
      const epoch = writeEpochs.get(key) ?? 0;
      draftSequence += 1;
      const id = draftSequence;
      update(key, (entry) => ({
        ...entry,
        draft: { id, value, failed: false },
      }));

      const operation = (writeTails.get(key) ?? Promise.resolve(true)).then(
        async () => {
          if (epoch !== (writeEpochs.get(key) ?? 0)) {
            update(key, (entry) =>
              entry.draft?.id === id ? { ...entry, draft: null } : entry,
            );
            return false;
          }
          const revision = expectedRevision?.() ?? null;
          if (expectedRevision && revision === null) {
            update(key, (entry) =>
              entry.draft?.id === id ? { ...entry, draft: null } : entry,
            );
            return false;
          }
          const activeWrite = {
            value,
            expectedRevision: revision,
          };
          activeWrites.set(key, activeWrite);
          try {
            const result = await api.setStorageItem(key, value, revision);
            if (result.status === 'error') throw result.error;
            if (activeWrites.get(key) === activeWrite) activeWrites.delete(key);
            observedRevision = Math.max(observedRevision, result.data);
            update(key, (entry) => {
              const merged = mergeKvStorageSnapshot(entry, {
                value,
                revision: result.data,
              });
              return merged.draft?.id === id
                ? { ...merged, draft: null }
                : merged;
            });
            return true;
          } catch (error) {
            if (activeWrites.get(key) === activeWrite) activeWrites.delete(key);
            console.error('[useKvStorage] setStorageItem failed:', error);
            if (expectedRevision && isKvStorageRevisionConflict(error)) {
              writeEpochs.set(key, (writeEpochs.get(key) ?? 0) + 1);
              update(key, (entry) => ({ ...entry, draft: null }));
              void read(key);
              return false;
            }
            update(key, (entry) =>
              entry.draft?.id === id
                ? { ...entry, draft: { ...entry.draft, failed: true } }
                : entry,
            );
            if (expectedRevision) void read(key);
            return false;
          }
        },
      );
      writeTails.set(key, operation);
      return operation;
    },

    connect: () => {
      let disposed = false;
      let valueChangedUnlisten: (() => void) | null = null;
      let resyncUnlisten: (() => void) | null = null;
      let retryTimer: ReturnType<typeof setTimeout> | null = null;
      let retryDelay = Math.max(1, listenerRetryInitialDelayMs);
      let isReady = false;
      const onResync = () => {
        if (!disposed) resync();
      };
      const onValueChanged = ({
        payload,
      }: {
        payload: StorageValueChangedEvent;
      }) => {
        if (disposed) return;
        const change = kvStorageEventSnapshot(payload);
        observedRevision = Math.max(observedRevision, payload.revision);
        // A key this client never read is read when it is first used.
        if (!change || !peek(change.key)) return;
        applySnapshot(change.key, change.snapshot);
      };

      const registerMissingListeners = async () => {
        const pending: Promise<void>[] = [];
        if (!valueChangedUnlisten) {
          pending.push(
            api
              .listenStorageValueChanged(onValueChanged)
              .then((unlisten) => {
                if (disposed) {
                  Promise.resolve().then(unlisten).catch(console.error);
                } else {
                  valueChangedUnlisten = unlisten;
                }
              })
              .catch((error) => {
                console.error(
                  '[useKvStorage] storage value listener registration failed; retrying:',
                  error,
                );
              }),
          );
        }
        if (!resyncUnlisten) {
          pending.push(
            api
              .listenStorageResync(onResync)
              .then((unlisten) => {
                if (disposed) {
                  Promise.resolve().then(unlisten).catch(console.error);
                } else {
                  resyncUnlisten = unlisten;
                }
              })
              .catch((error) => {
                console.error(
                  '[useKvStorage] storage resync listener registration failed; retrying:',
                  error,
                );
              }),
          );
        }
        await Promise.all(pending);
        if (disposed) return;

        if (valueChangedUnlisten && resyncUnlisten) {
          if (!isReady) {
            isReady = true;
            // Initial reads have waited for both listeners, so they already
            // observe commits made while registration was unavailable.
            markReady();
          }
          return;
        }

        retryTimer = setTimeout(() => {
          retryTimer = null;
          retryDelay = Math.min(30_000, retryDelay * 2);
          void registerMissingListeners();
        }, retryDelay);
      };

      void registerMissingListeners();

      return () => {
        disposed = true;
        if (retryTimer) clearTimeout(retryTimer);
        for (const unlisten of [valueChangedUnlisten, resyncUnlisten]) {
          if (unlisten) Promise.resolve().then(unlisten).catch(console.error);
        }
      };
    },
  };
}

const KvStorageContext = createContext<KvStorageStore | null>(null);

export const KvStorageProvider = ({ children }: PropsWithChildren) => {
  const queryClient = useQueryClient();

  const store = useMemo(() => {
    // Only the store's reads and events change these values, and a key read
    // once is kept for as long as the window lives.
    queryClient.setQueryDefaults(['kvStorage'], {
      queryFn: skipToken,
      gcTime: Infinity,
    });
    return createKvStorageStore(queryClient);
  }, [queryClient]);

  useEffect(() => store.connect(), [store]);

  return (
    <KvStorageContext.Provider value={store}>
      {children}
    </KvStorageContext.Provider>
  );
};

export const useKvStorageStore = () => {
  const store = useContext(KvStorageContext);
  if (!store) {
    throw new Error('useKvStorage must be used within a KvStorageProvider');
  }
  return store;
};
