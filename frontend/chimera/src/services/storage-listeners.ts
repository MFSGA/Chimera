export type StorageListenerEvent<T> = { payload: T };
export type StorageUnlisten = () => void | Promise<void>;
export type StorageListen = <T>(
  event: string,
  handler: (event: StorageListenerEvent<T>) => void,
) => Promise<StorageUnlisten>;

interface StorageListenerOptions {
  listen: StorageListen;
  onStorageValueChanged: (payload: [string, string | null]) => void;
  onStorageResyncRequired: (skipped: number) => void;
  onRegistrationError?: (event: string, error: unknown) => void;
  onEventError?: (event: string, error: unknown) => void;
  onCleanupError?: (event: string, error: unknown) => void;
  registrationMaxRetries?: number;
  registrationRetryDelayMs?: number;
  sleep?: (delayMs: number, signal: AbortSignal) => Promise<void>;
}

export interface StorageListenerSubscription {
  dispose: () => void;
  disposeAsync: () => Promise<void>;
}

const DEFAULT_REGISTRATION_MAX_RETRIES = 1;
const MAX_REGISTRATION_MAX_RETRIES = 10;
const DEFAULT_REGISTRATION_RETRY_DELAY_MS = 250;
const MIN_REGISTRATION_RETRY_DELAY_MS = 50;
const MAX_REGISTRATION_RETRY_DELAY_MS = 30_000;

function normalizeRetryCount(value: number): number {
  if (!Number.isFinite(value)) return DEFAULT_REGISTRATION_MAX_RETRIES;
  return Math.min(MAX_REGISTRATION_MAX_RETRIES, Math.max(0, Math.trunc(value)));
}

function normalizeRetryDelay(value: number): number {
  if (!Number.isFinite(value)) return DEFAULT_REGISTRATION_RETRY_DELAY_MS;
  return Math.min(
    MAX_REGISTRATION_RETRY_DELAY_MS,
    Math.max(MIN_REGISTRATION_RETRY_DELAY_MS, Math.trunc(value)),
  );
}

function sleepWithAbort(delayMs: number, signal: AbortSignal): Promise<void> {
  return new Promise((resolve) => {
    const finish = () => {
      signal.removeEventListener('abort', abort);
      resolve();
    };
    const abort = () => {
      clearTimeout(timer);
      finish();
    };
    const timer = setTimeout(finish, delayMs);
    signal.addEventListener('abort', abort, { once: true });
  });
}

/** Register both storage listeners and make asynchronous cleanup idempotent. */
export function createStorageListenerSubscription({
  listen,
  onStorageValueChanged,
  onStorageResyncRequired,
  onRegistrationError = () => undefined,
  onEventError = () => undefined,
  onCleanupError = () => undefined,
  registrationMaxRetries = DEFAULT_REGISTRATION_MAX_RETRIES,
  registrationRetryDelayMs = DEFAULT_REGISTRATION_RETRY_DELAY_MS,
  sleep = sleepWithAbort,
}: StorageListenerOptions): StorageListenerSubscription {
  const unlisteners = new Map<StorageUnlisten, string>();
  const cleanupTasks = new Set<Promise<void>>();
  const registrationTasks = new Set<Promise<void>>();
  const retryAbortController = new AbortController();
  const maxRetries = normalizeRetryCount(registrationMaxRetries);
  const retryDelayMs = normalizeRetryDelay(registrationRetryDelayMs);
  let disposed = false;
  let malformedResyncScheduled = false;

  const scheduleMalformedResync = () => {
    if (malformedResyncScheduled) return;
    malformedResyncScheduled = true;
    queueMicrotask(() => {
      malformedResyncScheduled = false;
      if (!disposed) onStorageResyncRequired(0);
    });
  };

  const reportCleanupError = (event: string, error: unknown) => {
    try {
      onCleanupError(event, error);
    } catch {
      // Cleanup diagnostics must never interrupt remaining resource release.
    }
  };

  const safeUnlisten = (event: string, unlisten: StorageUnlisten) => {
    let cleanupTask: Promise<void>;
    try {
      cleanupTask = Promise.resolve(unlisten()).catch((error) => {
        reportCleanupError(event, error);
      });
    } catch (error) {
      reportCleanupError(event, error);
      cleanupTask = Promise.resolve();
    }
    cleanupTasks.add(cleanupTask);
    void cleanupTask.finally(() => cleanupTasks.delete(cleanupTask));
    return cleanupTask;
  };

  const register = <T>(event: string, handler: (payload: T) => void): void => {
    const eventHandler = ({ payload }: StorageListenerEvent<T>) => {
      if (disposed) return;
      try {
        handler(payload);
      } catch (error) {
        onEventError(event, error);
        if (event === 'storage_value_changed') scheduleMalformedResync();
      }
    };

    const attemptRegistration = async () => {
      let attempt = 0;
      while (!disposed) {
        try {
          const unlisten = await listen<T>(event, eventHandler);
          if (disposed) safeUnlisten(event, unlisten);
          else unlisteners.set(unlisten, event);
          return;
        } catch (error) {
          if (disposed) return;
          if (attempt >= maxRetries) {
            onRegistrationError(event, error);
            return;
          }
          attempt += 1;
          await sleep(retryDelayMs, retryAbortController.signal);
        }
      }
    };

    const registrationTask = attemptRegistration();
    registrationTasks.add(registrationTask);
    void registrationTask.finally(() =>
      registrationTasks.delete(registrationTask),
    );
  };

  register<unknown>('storage_value_changed', (payload) => {
    if (
      !Array.isArray(payload) ||
      payload.length !== 2 ||
      typeof payload[0] !== 'string' ||
      (typeof payload[1] !== 'string' && payload[1] !== null)
    ) {
      throw new TypeError('invalid storage_value_changed payload');
    }
    onStorageValueChanged([payload[0], payload[1]]);
  });
  register<unknown>('storage_resync_required', (payload) => {
    if (
      typeof payload !== 'number' ||
      !Number.isSafeInteger(payload) ||
      payload < 0
    ) {
      throw new TypeError('invalid storage_resync_required payload');
    }
    onStorageResyncRequired(payload);
  });

  const dispose = () => {
    if (disposed) return;
    disposed = true;
    retryAbortController.abort();
    const registered = [...unlisteners.entries()];
    unlisteners.clear();
    registered.forEach(([unlisten, event]) => safeUnlisten(event, unlisten));
  };

  return {
    dispose,
    async disposeAsync() {
      dispose();
      await Promise.all([...registrationTasks]);
      await Promise.all([...cleanupTasks]);
    },
  };
}
