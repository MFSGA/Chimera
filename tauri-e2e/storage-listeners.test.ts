import assert from 'node:assert/strict';
import test from 'node:test';
import {
  createStorageListenerSubscription,
  type StorageListen,
  type StorageListenerEvent,
  type StorageUnlisten,
} from '../frontend/chimera/src/services/storage-listeners.js';

function deferred<T>() {
  let resolve!: (value: T) => void;
  let reject!: (error: unknown) => void;
  const promise = new Promise<T>((resolvePromise, rejectPromise) => {
    resolve = resolvePromise;
    reject = rejectPromise;
  });
  return { promise, reject, resolve };
}

test('storage listeners unregister exactly once when registration resolves after disposal', async () => {
  const registrations = new Map<
    string,
    ReturnType<typeof deferred<StorageUnlisten>>
  >();
  const handlers = new Map<
    string,
    (event: StorageListenerEvent<unknown>) => void
  >();
  const unlistenCalls: string[] = [];
  const valueChanges: unknown[] = [];
  let resyncCalls = 0;

  const listen: StorageListen = (event, handler) => {
    const registration = deferred<StorageUnlisten>();
    registrations.set(event, registration);
    handlers.set(
      event,
      handler as (event: StorageListenerEvent<unknown>) => void,
    );
    return registration.promise;
  };

  const subscription = createStorageListenerSubscription({
    listen,
    onStorageValueChanged: (payload) => valueChanges.push(payload),
    onStorageResyncRequired: () => resyncCalls++,
  });

  subscription.dispose();
  subscription.dispose();

  handlers.get('storage_value_changed')?.({ payload: ['web:route', '"/"'] });
  handlers.get('storage_resync_required')?.({ payload: 3 });
  assert.deepEqual(valueChanges, []);
  assert.equal(resyncCalls, 0);

  registrations.get('storage_resync_required')?.resolve(() => {
    unlistenCalls.push('resync');
  });
  await Promise.resolve();
  registrations.get('storage_value_changed')?.resolve(() => {
    unlistenCalls.push('value');
  });
  await Promise.resolve();

  assert.deepEqual(unlistenCalls, ['resync', 'value']);
  subscription.dispose();
  assert.deepEqual(unlistenCalls, ['resync', 'value']);
});

test('storage listeners clean up resolved and pending registrations in either order', async () => {
  const registrations = new Map<
    string,
    ReturnType<typeof deferred<StorageUnlisten>>
  >();
  const unlistenCounts = new Map<string, number>();

  const listen: StorageListen = (event) => {
    const registration = deferred<StorageUnlisten>();
    registrations.set(event, registration);
    return registration.promise;
  };

  const subscription = createStorageListenerSubscription({
    listen,
    onStorageValueChanged: () => undefined,
    onStorageResyncRequired: () => undefined,
  });
  const makeUnlisten = (event: string) => () => {
    unlistenCounts.set(event, (unlistenCounts.get(event) ?? 0) + 1);
  };

  registrations
    .get('storage_value_changed')
    ?.resolve(makeUnlisten('storage_value_changed'));
  await Promise.resolve();
  subscription.dispose();
  registrations
    .get('storage_resync_required')
    ?.resolve(makeUnlisten('storage_resync_required'));
  await Promise.resolve();

  assert.deepEqual(Object.fromEntries(unlistenCounts), {
    storage_resync_required: 1,
    storage_value_changed: 1,
  });
});

test('storage listener validates malformed payloads, coalesces resync, and continues', async () => {
  const handlers = new Map<
    string,
    (event: StorageListenerEvent<unknown>) => void
  >();
  const valueChanges: unknown[] = [];
  const eventErrors: Array<[string, unknown]> = [];
  let resyncCalls = 0;

  const listen: StorageListen = async (event, handler) => {
    handlers.set(
      event,
      handler as (event: StorageListenerEvent<unknown>) => void,
    );
    return () => undefined;
  };

  createStorageListenerSubscription({
    listen,
    onStorageValueChanged: ([key, value]) => {
      valueChanges.push([
        key,
        typeof value === 'string' ? JSON.parse(value) : value,
      ]);
    },
    onStorageResyncRequired: () => resyncCalls++,
    onEventError: (event, error) => eventErrors.push([event, error]),
  });
  await Promise.resolve();

  handlers.get('storage_value_changed')?.({ payload: { key: 'web:route' } });
  handlers.get('storage_value_changed')?.({ payload: [42, '"/invalid"'] });
  handlers.get('storage_value_changed')?.({
    payload: ['web:route', '{broken-json'],
  });
  handlers.get('storage_value_changed')?.({
    payload: ['web:route', '"/settings"'],
  });
  await Promise.resolve();

  assert.equal(eventErrors.length, 3);
  assert.deepEqual(
    eventErrors.map(([event]) => event),
    ['storage_value_changed', 'storage_value_changed', 'storage_value_changed'],
  );
  assert.match(String(eventErrors[0]?.[1]), /invalid storage_value_changed/);
  assert.match(String(eventErrors[1]?.[1]), /invalid storage_value_changed/);
  assert.match(String(eventErrors[2]?.[1]), /JSON/);
  assert.equal(resyncCalls, 1);
  assert.deepEqual(valueChanges, [['web:route', '/settings']]);
});

test('storage listener rejects invalid resync payloads and accepts a later valid signal', async () => {
  const handlers = new Map<
    string,
    (event: StorageListenerEvent<unknown>) => void
  >();
  const eventErrors: Array<[string, unknown]> = [];
  const resyncPayloads: number[] = [];

  const listen: StorageListen = async (event, handler) => {
    handlers.set(
      event,
      handler as (event: StorageListenerEvent<unknown>) => void,
    );
    return () => undefined;
  };

  createStorageListenerSubscription({
    listen,
    onStorageValueChanged: () => undefined,
    onStorageResyncRequired: (skipped) => resyncPayloads.push(skipped),
    onEventError: (event, error) => eventErrors.push([event, error]),
  });
  await Promise.resolve();

  handlers.get('storage_resync_required')?.({ payload: '4' });
  handlers.get('storage_resync_required')?.({ payload: -1 });
  handlers.get('storage_resync_required')?.({ payload: 1.5 });
  handlers.get('storage_resync_required')?.({
    payload: Number.MAX_SAFE_INTEGER + 1,
  });

  assert.deepEqual(resyncPayloads, []);
  assert.equal(eventErrors.length, 4);
  assert.deepEqual(
    eventErrors.map(([event]) => event),
    Array(4).fill('storage_resync_required'),
  );
  eventErrors.forEach(([, error]) => {
    assert.match(String(error), /invalid storage_resync_required payload/);
  });

  handlers.get('storage_resync_required')?.({ payload: 0 });
  handlers.get('storage_resync_required')?.({ payload: 7 });

  assert.deepEqual(resyncPayloads, [0, 7]);
});

test('storage listener keeps the successful registration when its peer fails', async () => {
  const registrations = new Map<
    string,
    ReturnType<typeof deferred<StorageUnlisten>>
  >();
  const handlers = new Map<
    string,
    (event: StorageListenerEvent<unknown>) => void
  >();
  const registrationErrors: Array<[string, unknown]> = [];
  const valueChanges: Array<[string, string | null]> = [];
  let valueUnlistenCalls = 0;
  let resyncCalls = 0;

  const listen: StorageListen = (event, handler) => {
    const registration = deferred<StorageUnlisten>();
    registrations.set(event, registration);
    handlers.set(
      event,
      handler as (event: StorageListenerEvent<unknown>) => void,
    );
    return registration.promise;
  };

  const subscription = createStorageListenerSubscription({
    listen,
    onStorageValueChanged: (payload) => valueChanges.push(payload),
    onStorageResyncRequired: () => resyncCalls++,
    onRegistrationError: (event, error) =>
      registrationErrors.push([event, error]),
    registrationMaxRetries: 0,
  });

  registrations.get('storage_value_changed')?.resolve(() => {
    valueUnlistenCalls++;
  });
  registrations
    .get('storage_resync_required')
    ?.reject(new Error('resync registration unavailable'));
  await Promise.resolve();
  await Promise.resolve();

  assert.equal(registrationErrors.length, 1);
  assert.equal(registrationErrors[0]?.[0], 'storage_resync_required');
  assert.match(String(registrationErrors[0]?.[1]), /registration unavailable/);

  handlers.get('storage_value_changed')?.({
    payload: ['web:route', '"/profiles"'],
  });

  assert.deepEqual(valueChanges, [['web:route', '"/profiles"']]);
  assert.equal(resyncCalls, 0);

  subscription.dispose();
  subscription.dispose();
  assert.equal(valueUnlistenCalls, 1);

  handlers.get('storage_value_changed')?.({
    payload: ['web:route', '"/settings"'],
  });
  assert.deepEqual(valueChanges, [['web:route', '"/profiles"']]);
});

test('storage listener retries a transient registration failure with backoff', async () => {
  const handlers = new Map<
    string,
    (event: StorageListenerEvent<unknown>) => void
  >();
  const attempts = new Map<string, number>();
  const pendingSleeps: Array<() => void> = [];
  const registrationErrors: Array<[string, unknown]> = [];
  const valueChanges: Array<[string, string | null]> = [];
  let valueUnlistenCalls = 0;

  const listen: StorageListen = async (event, handler) => {
    const attempt = (attempts.get(event) ?? 0) + 1;
    attempts.set(event, attempt);
    if (event === 'storage_value_changed' && attempt === 1) {
      throw new Error('temporary value registration failure');
    }
    handlers.set(
      event,
      handler as (event: StorageListenerEvent<unknown>) => void,
    );
    return () => {
      if (event === 'storage_value_changed') valueUnlistenCalls += 1;
    };
  };

  const subscription = createStorageListenerSubscription({
    listen,
    onStorageValueChanged: (payload) => valueChanges.push(payload),
    onStorageResyncRequired: () => undefined,
    onRegistrationError: (event, error) =>
      registrationErrors.push([event, error]),
    registrationMaxRetries: 1,
    registrationRetryDelayMs: 250,
    sleep: async () =>
      new Promise<void>((resolve) => {
        pendingSleeps.push(resolve);
      }),
  });
  await Promise.resolve();

  assert.equal(attempts.get('storage_value_changed'), 1);
  assert.equal(pendingSleeps.length, 1);
  assert.deepEqual(registrationErrors, []);

  pendingSleeps.shift()?.();
  await new Promise<void>((resolve) => setImmediate(resolve));

  assert.equal(attempts.get('storage_value_changed'), 2);
  handlers.get('storage_value_changed')?.({
    payload: ['web:route', '"/recovered"'],
  });
  assert.deepEqual(valueChanges, [['web:route', '"/recovered"']]);
  assert.deepEqual(registrationErrors, []);

  subscription.dispose();
  assert.equal(valueUnlistenCalls, 1);
});

test('storage listener cancels registration backoff after disposal', async () => {
  const attempts = new Map<string, number>();
  const sleepSignals: AbortSignal[] = [];
  const registrationErrors: Array<[string, unknown]> = [];

  const listen: StorageListen = async (event) => {
    attempts.set(event, (attempts.get(event) ?? 0) + 1);
    if (event === 'storage_value_changed') {
      throw new Error('value registration unavailable');
    }
    return () => undefined;
  };

  const subscription = createStorageListenerSubscription({
    listen,
    onStorageValueChanged: () => undefined,
    onStorageResyncRequired: () => undefined,
    onRegistrationError: (event, error) =>
      registrationErrors.push([event, error]),
    registrationMaxRetries: 2,
    sleep: async (_delayMs, signal) => {
      sleepSignals.push(signal);
      await new Promise<void>((resolve) => {
        signal.addEventListener('abort', () => resolve(), { once: true });
      });
    },
  });
  await Promise.resolve();

  assert.equal(attempts.get('storage_value_changed'), 1);
  assert.equal(sleepSignals.length, 1);
  assert.equal(sleepSignals[0]?.aborted, false);

  subscription.dispose();
  assert.equal(sleepSignals[0]?.aborted, true);
  await new Promise<void>((resolve) => setImmediate(resolve));

  assert.equal(attempts.get('storage_value_changed'), 1);
  assert.deepEqual(registrationErrors, []);
});

test('storage listeners retry independently when both registrations initially fail', async () => {
  const handlers = new Map<
    string,
    (event: StorageListenerEvent<unknown>) => void
  >();
  const attempts = new Map<string, number>();
  const pendingSleeps: Array<() => void> = [];
  const registrationErrors: Array<[string, unknown]> = [];
  const valueChanges: Array<[string, string | null]> = [];
  let valueUnlistenCalls = 0;

  const listen: StorageListen = async (event, handler) => {
    const attempt = (attempts.get(event) ?? 0) + 1;
    attempts.set(event, attempt);

    if (event === 'storage_resync_required' || attempt === 1) {
      throw new Error(`${event} registration attempt ${attempt} failed`);
    }

    handlers.set(
      event,
      handler as (event: StorageListenerEvent<unknown>) => void,
    );
    return () => {
      if (event === 'storage_value_changed') valueUnlistenCalls += 1;
    };
  };

  const subscription = createStorageListenerSubscription({
    listen,
    onStorageValueChanged: (payload) => valueChanges.push(payload),
    onStorageResyncRequired: () => undefined,
    onRegistrationError: (event, error) =>
      registrationErrors.push([event, error]),
    registrationMaxRetries: 2,
    sleep: async () =>
      new Promise<void>((resolve) => {
        pendingSleeps.push(resolve);
      }),
  });
  await Promise.resolve();

  assert.deepEqual(Object.fromEntries(attempts), {
    storage_resync_required: 1,
    storage_value_changed: 1,
  });
  assert.equal(pendingSleeps.length, 2);

  pendingSleeps.shift()?.();
  await new Promise<void>((resolve) => setImmediate(resolve));

  assert.equal(attempts.get('storage_value_changed'), 2);
  handlers.get('storage_value_changed')?.({
    payload: ['web:route', '"/independent-recovery"'],
  });
  assert.deepEqual(valueChanges, [['web:route', '"/independent-recovery"']]);
  assert.deepEqual(registrationErrors, []);

  pendingSleeps.shift()?.();
  await new Promise<void>((resolve) => setImmediate(resolve));
  assert.equal(attempts.get('storage_resync_required'), 2);
  assert.equal(pendingSleeps.length, 1);
  assert.deepEqual(registrationErrors, []);

  pendingSleeps.shift()?.();
  await new Promise<void>((resolve) => setImmediate(resolve));

  assert.equal(attempts.get('storage_resync_required'), 3);
  assert.equal(registrationErrors.length, 1);
  assert.equal(registrationErrors[0]?.[0], 'storage_resync_required');
  assert.match(String(registrationErrors[0]?.[1]), /attempt 3 failed/);

  subscription.dispose();
  assert.equal(valueUnlistenCalls, 1);
});

test('storage listener cancels both registration backoffs after disposal', async () => {
  const attempts = new Map<string, number>();
  const sleepSignals = new Map<string, AbortSignal>();
  const registrationErrors: Array<[string, unknown]> = [];

  const subscription = createStorageListenerSubscription({
    listen: async (event) => {
      attempts.set(event, (attempts.get(event) ?? 0) + 1);
      throw new Error(`${event} unavailable`);
    },
    onStorageValueChanged: () => undefined,
    onStorageResyncRequired: () => undefined,
    onRegistrationError: (event, error) =>
      registrationErrors.push([event, error]),
    registrationMaxRetries: 2,
    sleep: async (_delayMs, signal) => {
      const event = sleepSignals.has('storage_value_changed')
        ? 'storage_resync_required'
        : 'storage_value_changed';
      sleepSignals.set(event, signal);
      await new Promise<void>((resolve) => {
        signal.addEventListener('abort', () => resolve(), { once: true });
      });
    },
  });
  await Promise.resolve();

  assert.deepEqual(Object.fromEntries(attempts), {
    storage_resync_required: 1,
    storage_value_changed: 1,
  });
  assert.equal(sleepSignals.size, 2);
  assert.equal(sleepSignals.get('storage_value_changed')?.aborted, false);
  assert.equal(sleepSignals.get('storage_resync_required')?.aborted, false);

  subscription.dispose();
  subscription.dispose();

  assert.equal(sleepSignals.get('storage_value_changed')?.aborted, true);
  assert.equal(sleepSignals.get('storage_resync_required')?.aborted, true);
  await new Promise<void>((resolve) => setImmediate(resolve));

  assert.deepEqual(Object.fromEntries(attempts), {
    storage_resync_required: 1,
    storage_value_changed: 1,
  });
  assert.deepEqual(registrationErrors, []);
});

test('storage listener clears default retry timers after disposal', async () => {
  const originalSetTimeout = globalThis.setTimeout;
  const originalClearTimeout = globalThis.clearTimeout;
  const callbacks = new Map<number, () => void>();
  const clearedTimers: number[] = [];
  const attempts = new Map<string, number>();
  let nextTimerId = 1;

  globalThis.setTimeout = ((callback: () => void) => {
    const timerId = nextTimerId++;
    callbacks.set(timerId, callback);
    return timerId;
  }) as typeof setTimeout;
  globalThis.clearTimeout = ((timerId: number) => {
    clearedTimers.push(timerId);
    callbacks.delete(timerId);
  }) as typeof clearTimeout;

  try {
    const subscription = createStorageListenerSubscription({
      listen: async (event) => {
        attempts.set(event, (attempts.get(event) ?? 0) + 1);
        throw new Error(`${event} unavailable`);
      },
      onStorageValueChanged: () => undefined,
      onStorageResyncRequired: () => undefined,
      registrationMaxRetries: 2,
    });
    await Promise.resolve();

    assert.deepEqual(Object.fromEntries(attempts), {
      storage_resync_required: 1,
      storage_value_changed: 1,
    });
    assert.equal(callbacks.size, 2);

    const lateCallbacks = [...callbacks.values()];
    subscription.dispose();

    assert.equal(callbacks.size, 0);
    assert.deepEqual(
      clearedTimers.sort((left, right) => left - right),
      [1, 2],
    );

    lateCallbacks.forEach((callback) => callback());
    await new Promise<void>((resolve) => setImmediate(resolve));

    assert.deepEqual(Object.fromEntries(attempts), {
      storage_resync_required: 1,
      storage_value_changed: 1,
    });
  } finally {
    globalThis.setTimeout = originalSetTimeout;
    globalThis.clearTimeout = originalClearTimeout;
  }
});

test('storage listener recovers when default retry timer expires', async () => {
  const originalSetTimeout = globalThis.setTimeout;
  const originalClearTimeout = globalThis.clearTimeout;
  const callbacks = new Map<number, () => void>();
  const attempts = new Map<string, number>();
  const clearedTimers: number[] = [];
  let nextTimerId = 1;
  let valueUnlistenCalls = 0;

  globalThis.setTimeout = ((callback: () => void) => {
    const timerId = nextTimerId++;
    callbacks.set(timerId, callback);
    return timerId;
  }) as typeof setTimeout;
  globalThis.clearTimeout = ((timerId: number) => {
    clearedTimers.push(timerId);
    callbacks.delete(timerId);
  }) as typeof clearTimeout;

  try {
    const subscription = createStorageListenerSubscription({
      listen: async (event) => {
        const attempt = (attempts.get(event) ?? 0) + 1;
        attempts.set(event, attempt);
        if (event === 'storage_value_changed' && attempt === 1) {
          throw new Error('temporary value registration failure');
        }
        return () => {
          if (event === 'storage_value_changed') valueUnlistenCalls += 1;
        };
      },
      onStorageValueChanged: () => undefined,
      onStorageResyncRequired: () => undefined,
      registrationMaxRetries: 1,
    });
    await Promise.resolve();

    assert.equal(attempts.get('storage_value_changed'), 1);
    assert.equal(callbacks.size, 1);

    const [timerId, callback] = [...callbacks.entries()][0] ?? [];
    assert.equal(typeof callback, 'function');
    callbacks.delete(timerId as number);
    callback?.();
    await new Promise<void>((resolve) => setImmediate(resolve));

    assert.equal(attempts.get('storage_value_changed'), 2);
    assert.equal(callbacks.size, 0);
    assert.deepEqual(clearedTimers, []);

    subscription.dispose();
    subscription.dispose();
    assert.equal(valueUnlistenCalls, 1);
    assert.deepEqual(clearedTimers, []);
  } finally {
    globalThis.setTimeout = originalSetTimeout;
    globalThis.clearTimeout = originalClearTimeout;
  }
});

test('storage listener normalizes invalid retry configuration', async () => {
  const runScenario = async ({
    retries,
    delay,
  }: {
    retries: number;
    delay: number;
  }) => {
    let valueAttempts = 0;
    const sleepDelays: number[] = [];
    const registrationErrors: Array<[string, unknown]> = [];

    createStorageListenerSubscription({
      listen: async (event) => {
        if (event === 'storage_value_changed') {
          valueAttempts += 1;
          throw new Error(`value attempt ${valueAttempts} failed`);
        }
        return () => undefined;
      },
      onStorageValueChanged: () => undefined,
      onStorageResyncRequired: () => undefined,
      onRegistrationError: (event, error) =>
        registrationErrors.push([event, error]),
      registrationMaxRetries: retries,
      registrationRetryDelayMs: delay,
      sleep: async (delayMs) => {
        sleepDelays.push(delayMs);
      },
    });

    await new Promise<void>((resolve) => setImmediate(resolve));
    return { valueAttempts, sleepDelays, registrationErrors };
  };

  const negative = await runScenario({ retries: -3, delay: -20 });
  assert.equal(negative.valueAttempts, 1);
  assert.deepEqual(negative.sleepDelays, []);
  assert.equal(negative.registrationErrors.length, 1);

  const fractional = await runScenario({ retries: 1.9, delay: 112.8 });
  assert.equal(fractional.valueAttempts, 2);
  assert.deepEqual(fractional.sleepDelays, [112]);
  assert.equal(fractional.registrationErrors.length, 1);

  const zeroDelay = await runScenario({ retries: 1, delay: 0 });
  assert.equal(zeroDelay.valueAttempts, 2);
  assert.deepEqual(zeroDelay.sleepDelays, [50]);
  assert.equal(zeroDelay.registrationErrors.length, 1);

  const excessive = await runScenario({
    retries: Number.MAX_SAFE_INTEGER,
    delay: Number.MAX_SAFE_INTEGER,
  });
  assert.equal(excessive.valueAttempts, 11);
  assert.deepEqual(excessive.sleepDelays, Array(10).fill(30_000));
  assert.equal(excessive.registrationErrors.length, 1);

  for (const retries of [Number.NaN, Number.POSITIVE_INFINITY]) {
    const nonFinite = await runScenario({
      retries,
      delay: Number.POSITIVE_INFINITY,
    });
    assert.equal(nonFinite.valueAttempts, 2);
    assert.deepEqual(nonFinite.sleepDelays, [250]);
    assert.equal(nonFinite.registrationErrors.length, 1);
  }
});

test('disposed storage listeners ignore late registration failures', async () => {
  const registrations = new Map<
    string,
    ReturnType<typeof deferred<StorageUnlisten>>
  >();
  const errors: Array<[string, unknown]> = [];

  const listen: StorageListen = (event) => {
    const registration = deferred<StorageUnlisten>();
    registrations.set(event, registration);
    return registration.promise;
  };

  const subscription = createStorageListenerSubscription({
    listen,
    onStorageValueChanged: () => undefined,
    onStorageResyncRequired: () => undefined,
    onRegistrationError: (event, error) => errors.push([event, error]),
  });

  subscription.dispose();
  registrations
    .get('storage_value_changed')
    ?.reject(new Error('late value failure'));
  registrations
    .get('storage_resync_required')
    ?.reject(new Error('late resync failure'));
  await Promise.resolve();
  await Promise.resolve();

  assert.deepEqual(errors, []);
});

test('storage listener isolates unlisten failures during disposal', async () => {
  const cleanupCalls: string[] = [];
  const cleanupErrors: Array<[string, unknown]> = [];
  const cleanupFailure = new Error('value cleanup failed');

  const listen: StorageListen = async (event) => {
    if (event === 'storage_value_changed') {
      return () => {
        cleanupCalls.push(event);
        throw cleanupFailure;
      };
    }
    return () => {
      cleanupCalls.push(event);
    };
  };

  const subscription = createStorageListenerSubscription({
    listen,
    onStorageValueChanged: () => undefined,
    onStorageResyncRequired: () => undefined,
    onCleanupError: (event, error) => cleanupErrors.push([event, error]),
  });
  await Promise.resolve();

  subscription.dispose();
  subscription.dispose();

  assert.deepEqual(cleanupCalls, [
    'storage_value_changed',
    'storage_resync_required',
  ]);
  assert.deepEqual(cleanupErrors, [['storage_value_changed', cleanupFailure]]);
});

test('storage listener contains cleanup diagnostic failures', async () => {
  const cleanupCalls: string[] = [];
  const cleanupFailure = new Error('cleanup failed');
  let diagnosticCalls = 0;

  const listen: StorageListen = async (event) => () => {
    cleanupCalls.push(event);
    throw cleanupFailure;
  };

  const subscription = createStorageListenerSubscription({
    listen,
    onStorageValueChanged: () => undefined,
    onStorageResyncRequired: () => undefined,
    onCleanupError: () => {
      diagnosticCalls += 1;
      throw new Error('diagnostic sink failed');
    },
  });
  await Promise.resolve();

  assert.doesNotThrow(() => subscription.dispose());
  subscription.dispose();

  assert.deepEqual(cleanupCalls, [
    'storage_value_changed',
    'storage_resync_required',
  ]);
  assert.equal(diagnosticCalls, 2);
});

test('storage listener isolates late registration cleanup failures', async () => {
  const registrations = new Map<
    string,
    ReturnType<typeof deferred<StorageUnlisten>>
  >();
  const cleanupCalls: string[] = [];
  const cleanupErrors: Array<[string, unknown]> = [];
  const valueCleanupFailure = new Error('late value cleanup failed');
  const resyncCleanupFailure = new Error('late resync cleanup failed');

  const listen: StorageListen = (event) => {
    const registration = deferred<StorageUnlisten>();
    registrations.set(event, registration);
    return registration.promise;
  };

  const subscription = createStorageListenerSubscription({
    listen,
    onStorageValueChanged: () => undefined,
    onStorageResyncRequired: () => undefined,
    onCleanupError: (event, error) => cleanupErrors.push([event, error]),
  });

  subscription.dispose();
  registrations.get('storage_value_changed')?.resolve(() => {
    cleanupCalls.push('storage_value_changed');
    throw valueCleanupFailure;
  });
  registrations.get('storage_resync_required')?.resolve(() => {
    cleanupCalls.push('storage_resync_required');
    throw resyncCleanupFailure;
  });
  await Promise.resolve();
  await Promise.resolve();

  subscription.dispose();

  assert.deepEqual(cleanupCalls, [
    'storage_value_changed',
    'storage_resync_required',
  ]);
  assert.deepEqual(cleanupErrors, [
    ['storage_value_changed', valueCleanupFailure],
    ['storage_resync_required', resyncCleanupFailure],
  ]);
});

test('storage listener contains asynchronous unlisten rejections', async () => {
  const cleanupCalls: string[] = [];
  const cleanupErrors: Array<[string, unknown]> = [];
  const asyncCleanupFailure = new Error('async value cleanup failed');

  const listen: StorageListen = async (event) => {
    if (event === 'storage_value_changed') {
      return async () => {
        cleanupCalls.push(event);
        throw asyncCleanupFailure;
      };
    }
    return async () => {
      cleanupCalls.push(event);
      await Promise.resolve();
    };
  };

  const subscription = createStorageListenerSubscription({
    listen,
    onStorageValueChanged: () => undefined,
    onStorageResyncRequired: () => undefined,
    onCleanupError: (event, error) => cleanupErrors.push([event, error]),
  });
  await Promise.resolve();

  assert.doesNotThrow(() => subscription.dispose());
  subscription.dispose();
  await new Promise<void>((resolve) => setImmediate(resolve));

  assert.deepEqual(cleanupCalls, [
    'storage_value_changed',
    'storage_resync_required',
  ]);
  assert.deepEqual(cleanupErrors, [
    ['storage_value_changed', asyncCleanupFailure],
  ]);
});

test('storage listener contains late asynchronous cleanup rejections', async () => {
  const registrations = new Map<
    string,
    ReturnType<typeof deferred<StorageUnlisten>>
  >();
  const cleanupCalls: string[] = [];
  const cleanupErrors: Array<[string, unknown]> = [];
  const valueFailure = new Error('late async value cleanup failed');
  const resyncFailure = new Error('late async resync cleanup failed');

  const listen: StorageListen = (event) => {
    const registration = deferred<StorageUnlisten>();
    registrations.set(event, registration);
    return registration.promise;
  };

  const subscription = createStorageListenerSubscription({
    listen,
    onStorageValueChanged: () => undefined,
    onStorageResyncRequired: () => undefined,
    onCleanupError: (event, error) => cleanupErrors.push([event, error]),
  });

  subscription.dispose();
  registrations.get('storage_value_changed')?.resolve(async () => {
    cleanupCalls.push('storage_value_changed');
    await Promise.resolve();
    throw valueFailure;
  });
  registrations.get('storage_resync_required')?.resolve(async () => {
    cleanupCalls.push('storage_resync_required');
    await Promise.resolve();
    throw resyncFailure;
  });

  await new Promise<void>((resolve) => setImmediate(resolve));
  subscription.dispose();
  await new Promise<void>((resolve) => setImmediate(resolve));

  assert.deepEqual(cleanupCalls, [
    'storage_value_changed',
    'storage_resync_required',
  ]);
  assert.deepEqual(cleanupErrors, [
    ['storage_value_changed', valueFailure],
    ['storage_resync_required', resyncFailure],
  ]);
});

test('storage listener does not let a pending asynchronous cleanup block its peer', async () => {
  const pendingCleanup = deferred<void>();
  const cleanupCalls: string[] = [];
  const cleanupCompletions: string[] = [];
  const cleanupErrors: Array<[string, unknown]> = [];

  const listen: StorageListen = async (event) => {
    if (event === 'storage_value_changed') {
      return async () => {
        cleanupCalls.push(event);
        await pendingCleanup.promise;
        cleanupCompletions.push(event);
      };
    }
    return async () => {
      cleanupCalls.push(event);
      await Promise.resolve();
      cleanupCompletions.push(event);
    };
  };

  const subscription = createStorageListenerSubscription({
    listen,
    onStorageValueChanged: () => undefined,
    onStorageResyncRequired: () => undefined,
    onCleanupError: (event, error) => cleanupErrors.push([event, error]),
  });
  await Promise.resolve();

  assert.doesNotThrow(() => subscription.dispose());
  subscription.dispose();
  await new Promise<void>((resolve) => setImmediate(resolve));

  assert.deepEqual(cleanupCalls, [
    'storage_value_changed',
    'storage_resync_required',
  ]);
  assert.deepEqual(cleanupCompletions, ['storage_resync_required']);
  assert.deepEqual(cleanupErrors, []);

  pendingCleanup.resolve();
  await new Promise<void>((resolve) => setImmediate(resolve));

  assert.deepEqual(cleanupCompletions, [
    'storage_resync_required',
    'storage_value_changed',
  ]);
  assert.deepEqual(cleanupErrors, []);
  subscription.dispose();
  assert.deepEqual(cleanupCalls, [
    'storage_value_changed',
    'storage_resync_required',
  ]);
});

test('storage listener disposeAsync waits for pending and late cleanup exactly once', async () => {
  const registrations = new Map<
    string,
    ReturnType<typeof deferred<StorageUnlisten>>
  >();
  const pendingCleanup = deferred<void>();
  const cleanupCalls: string[] = [];
  const cleanupErrors: Array<[string, unknown]> = [];

  const listen: StorageListen = (event) => {
    const registration = deferred<StorageUnlisten>();
    registrations.set(event, registration);
    return registration.promise;
  };

  const subscription = createStorageListenerSubscription({
    listen,
    onStorageValueChanged: () => undefined,
    onStorageResyncRequired: () => undefined,
    onCleanupError: (event, error) => cleanupErrors.push([event, error]),
  });

  let disposed = false;
  const disposal = subscription.disposeAsync().then(() => {
    disposed = true;
  });

  registrations.get('storage_value_changed')?.resolve(async () => {
    cleanupCalls.push('storage_value_changed');
    await pendingCleanup.promise;
  });
  registrations.get('storage_resync_required')?.resolve(async () => {
    cleanupCalls.push('storage_resync_required');
    throw new Error('late cleanup rejected');
  });
  await new Promise<void>((resolve) => setImmediate(resolve));

  assert.equal(disposed, false);
  assert.deepEqual(cleanupCalls, [
    'storage_value_changed',
    'storage_resync_required',
  ]);
  assert.equal(cleanupErrors.length, 1);
  assert.equal(cleanupErrors[0]?.[0], 'storage_resync_required');

  pendingCleanup.resolve();
  await disposal;
  await subscription.disposeAsync();

  assert.equal(disposed, true);
  assert.deepEqual(cleanupCalls, [
    'storage_value_changed',
    'storage_resync_required',
  ]);
  assert.equal(cleanupErrors.length, 1);
});
