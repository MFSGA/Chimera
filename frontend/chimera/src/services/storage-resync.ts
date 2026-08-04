export type StorageSnapshot = Record<string, unknown>;

type StorageResyncOptions = {
  maxRetries?: number;
  retryDelayMs?: number;
  sleep?: (delayMs: number) => Promise<void>;
};

/** Coalesces snapshot reloads so only one request runs at a time. */
export function createStorageResyncCoordinator(
  loadSnapshot: () => Promise<StorageSnapshot>,
  applySnapshot: (snapshot: StorageSnapshot) => void,
  reportError: (error: unknown) => void,
  options: StorageResyncOptions = {},
) {
  const maxRetries = options.maxRetries ?? 1;
  const retryDelayMs = options.retryDelayMs ?? 250;
  const sleep =
    options.sleep ??
    ((delayMs) => new Promise((resolve) => setTimeout(resolve, delayMs)));
  let disposed = false;
  let rerunRequested = false;
  let running: Promise<void> | null = null;
  let wakeRetry: (() => void) | null = null;

  const waitForRetry = async () => {
    await Promise.race([
      sleep(retryDelayMs),
      new Promise<void>((resolve) => {
        wakeRetry = resolve;
      }),
    ]);
    wakeRetry = null;
  };

  const loadWithRetry = async () => {
    let lastError: unknown;
    for (let attempt = 0; attempt <= maxRetries && !disposed; attempt += 1) {
      try {
        return await loadSnapshot();
      } catch (error) {
        lastError = error;
        if (attempt < maxRetries) await waitForRetry();
      }
    }
    throw lastError;
  };

  const run = async () => {
    do {
      rerunRequested = false;
      try {
        const snapshot = await loadWithRetry();
        if (!disposed) applySnapshot(snapshot);
      } catch (error) {
        if (!disposed) reportError(error);
      }
    } while (!disposed && rerunRequested);
  };

  return {
    resync() {
      if (disposed) return Promise.resolve();
      if (running) {
        rerunRequested = true;
        wakeRetry?.();
        return running;
      }

      running = run().finally(() => {
        running = null;
      });
      return running;
    },
    dispose() {
      disposed = true;
      rerunRequested = false;
      wakeRetry?.();
    },
  };
}

/** Reconciles subscribed keys against a complete backend snapshot. */
export function reconcileStorageSnapshot(
  subscribedKeys: Iterable<string>,
  snapshot: StorageSnapshot,
  dispatch: (key: string, value: unknown | null) => void,
) {
  for (const key of subscribedKeys) {
    dispatch(
      key,
      Object.prototype.hasOwnProperty.call(snapshot, key)
        ? snapshot[key]
        : null,
    );
  }
}
