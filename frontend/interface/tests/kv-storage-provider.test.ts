import assert from 'node:assert/strict';
import test from 'node:test';
import { QueryClient } from '@tanstack/react-query';
import type {
  StorageResyncEvent,
  StorageValueChangedEvent,
} from '../src/ipc/bindings';
import {
  createKvStorageStore,
  type KvStorageApi,
} from '../src/provider/kv-storage-provider';
import {
  canWriteKvStorageValue,
  decodeKvStorageValue,
} from '../src/provider/kv-storage-state';

const tick = () => new Promise<void>((resolve) => setImmediate(resolve));
type StorageWriteResult = Awaited<ReturnType<KvStorageApi['setStorageItem']>>;

function deferred<T>() {
  let resolve!: (value: T) => void;
  const promise = new Promise<T>((done) => {
    resolve = done;
  });
  return { promise, resolve };
}

function createHarness(
  readStorageItem: KvStorageApi['getStorageItem'],
  writeStorageItem: KvStorageApi['setStorageItem'],
) {
  let onValueChanged:
    ((event: { payload: StorageValueChangedEvent }) => void) | undefined;
  let onResync: ((event: { payload: StorageResyncEvent }) => void) | undefined;
  const api: KvStorageApi = {
    getStorageItem: readStorageItem,
    setStorageItem: writeStorageItem,
    listenStorageValueChanged: async (handler) => {
      onValueChanged = handler;
      return () => undefined;
    },
    listenStorageResync: async (handler) => {
      onResync = handler;
      return () => undefined;
    },
  };
  const queryClient = new QueryClient({
    defaultOptions: { queries: { retry: false, gcTime: Infinity } },
  });
  const store = createKvStorageStore(queryClient, api);
  const disconnect = store.connect();

  return {
    store,
    disconnect,
    emitValueChanged(payload: StorageValueChangedEvent) {
      assert.ok(onValueChanged, 'storage listener has registered');
      onValueChanged({ payload });
    },
    emitResync(payload: StorageResyncEvent = {}) {
      assert.ok(onResync, 'resync listener has registered');
      onResync({ payload });
    },
    dispose() {
      disconnect();
      queryClient.clear();
    },
  };
}

test('newer events win over an older in-flight read and resync rereads used keys', async () => {
  const firstRead =
    deferred<Awaited<ReturnType<KvStorageApi['getStorageItem']>>>();
  let reads = 0;
  const harness = createHarness(
    () => {
      reads += 1;
      return reads === 1
        ? firstRead.promise
        : Promise.resolve({
            status: 'ok',
            data: { value: '{"source":"resync"}', revision: 7 },
          });
    },
    async () => ({ status: 'ok', data: 1 }),
  );

  try {
    harness.store.ensureRead('layout');
    harness.store.ensureRead('layout');
    await tick();
    assert.equal(reads, 1);

    harness.emitValueChanged({
      key: 'web:layout',
      value: JSON.stringify(JSON.stringify({ source: 'event' })),
      revision: 6,
    });
    firstRead.resolve({
      status: 'ok',
      data: { value: '{"source":"read"}', revision: 5 },
    });
    await harness.store.refresh('layout');
    assert.equal(harness.store.peek('layout')?.value, '{"source":"event"}');
    assert.equal(harness.store.peek('layout')?.revision, 6);
    assert.equal(harness.store.currentRevision(), 6);

    harness.emitResync();
    await tick();
    assert.equal(reads, 2);
    assert.equal(harness.store.peek('layout')?.value, '{"source":"resync"}');
    assert.equal(harness.store.peek('layout')?.revision, 7);
    assert.equal(harness.store.currentRevision(), 7);
  } finally {
    harness.dispose();
  }
});

test('same-key writes share one queue and the latest draft across consumers', async () => {
  const writes: Array<{
    promise: Promise<StorageWriteResult>;
    resolve: (value: StorageWriteResult) => void;
    value: string;
  }> = [];
  const harness = createHarness(
    async () => ({
      status: 'ok',
      data: { value: '"initial"', revision: 0 },
    }),
    (_key, value) => {
      const result = deferred<StorageWriteResult>();
      writes.push(Object.assign(result, { value }));
      return result.promise;
    },
  );

  try {
    harness.store.ensureRead('shared');
    await harness.store.refresh('shared');

    const first = harness.store.write('shared', '"first"');
    const second = harness.store.write('shared', '"second"');
    await tick();

    assert.equal(writes.length, 1);
    assert.equal(writes[0].value, '"first"');
    assert.equal(harness.store.peek('shared')?.draft?.value, '"second"');

    writes[0].resolve({ status: 'ok', data: 1 });
    assert.equal(await first, true);
    await tick();
    assert.equal(writes.length, 2);
    assert.equal(writes[1].value, '"second"');

    writes[1].resolve({ status: 'ok', data: 2 });
    assert.equal(await second, true);
    assert.deepEqual(
      {
        value: harness.store.peek('shared')?.value,
        revision: harness.store.peek('shared')?.revision,
        draft: harness.store.peek('shared')?.draft,
      },
      { value: '"second"', revision: 2, draft: null },
    );
  } finally {
    harness.dispose();
  }
});

test('conditional writes use the latest revision observed by any storage event', async () => {
  const expectedRevisions: Array<number | null> = [];
  let writeRevision = 6;
  const harness = createHarness(
    async () => ({
      status: 'ok',
      data: { value: '"initial"', revision: 4 },
    }),
    async (_key, _value, expectedRevision) => {
      expectedRevisions.push(expectedRevision);
      const revision = writeRevision;
      writeRevision += 1;
      return { status: 'ok', data: revision };
    },
  );

  try {
    harness.store.ensureRead('shared');
    await harness.store.refresh('shared');
    harness.emitValueChanged({
      key: 'web:another-key',
      value: JSON.stringify('"other"'),
      revision: 5,
    });

    const saved = await harness.store.write('shared', '"next"', () =>
      harness.store.currentRevision(),
    );
    const savedAgain = await harness.store.write('shared', '"last"', () =>
      harness.store.currentRevision(),
    );

    assert.equal(saved, true);
    assert.equal(savedAgain, true);
    assert.deepEqual(expectedRevisions, [5, 6]);
    assert.equal(harness.store.currentRevision(), 7);
  } finally {
    harness.dispose();
  }
});

test('queued writes recheck migrations after a newer external revision arrives', async () => {
  const firstWrite = deferred<StorageWriteResult>();
  const writes: string[] = [];
  const defaultValue = { version: 1 };
  const migrate = (value: unknown) => {
    if ((value as { version?: number }).version !== 1) {
      throw new Error('unsupported storage version');
    }
    return value as typeof defaultValue;
  };
  const harness = createHarness(
    async () => ({
      status: 'ok',
      data: { value: '{"version":1}', revision: 1 },
    }),
    (_key, value) => {
      writes.push(value);
      return writes.length === 1
        ? firstWrite.promise
        : Promise.resolve({ status: 'ok', data: 3 });
    },
  );

  const canWrite = () => {
    const current = harness.store.peek('widgets');
    if (!current || current.value === undefined) return null;
    if (
      !canWriteKvStorageValue(
        current,
        decodeKvStorageValue(current.value, defaultValue, migrate).error,
      )
    ) {
      return null;
    }
    return harness.store.currentRevision();
  };

  try {
    harness.store.ensureRead('widgets');
    await harness.store.refresh('widgets');
    const first = harness.store.write('widgets', '{"version":1,"n":1}');
    await tick();
    assert.equal(writes.length, 1);

    harness.emitValueChanged({
      key: 'web:widgets',
      value: JSON.stringify(JSON.stringify({ version: 2, n: 9 })),
      revision: 2,
    });
    const second = harness.store.write(
      'widgets',
      '{"version":1,"n":2}',
      canWrite,
    );

    firstWrite.resolve({ status: 'ok', data: 1 });
    assert.equal(await first, true);
    assert.equal(await second, false);
    assert.deepEqual(writes, ['{"version":1,"n":1}']);
    assert.equal(harness.store.peek('widgets')?.value, '{"version":2,"n":9}');
    assert.equal(harness.store.peek('widgets')?.draft, null);
  } finally {
    harness.dispose();
  }
});

test('a revision conflict drops a draft when its event already advanced the entry', async () => {
  const conflictingWrite = deferred<StorageWriteResult>();
  const writes: string[] = [];
  let reads = 0;
  const defaultValue = { version: 1 };
  const migrate = (value: unknown) => {
    if ((value as { version?: number }).version !== 1) {
      throw new Error('unsupported storage version');
    }
    return value as typeof defaultValue;
  };
  const harness = createHarness(
    async () => {
      reads += 1;
      return {
        status: 'ok',
        data:
          reads === 1
            ? { value: '{"version":1}', revision: 1 }
            : { value: '{"version":2,"from":"external"}', revision: 2 },
      };
    },
    (_key, value) => {
      writes.push(value);
      return conflictingWrite.promise;
    },
  );
  const expectedRevision = () => {
    const entry = harness.store.peek('widgets');
    if (!entry || entry.value === undefined) return null;
    if (
      !canWriteKvStorageValue(
        entry,
        decodeKvStorageValue(entry.value, defaultValue, migrate).error,
      )
    ) {
      return null;
    }
    return harness.store.currentRevision();
  };

  try {
    harness.store.ensureRead('widgets');
    await harness.store.refresh('widgets');
    const first = harness.store.write(
      'widgets',
      '{"version":1,"edited":"local"}',
      expectedRevision,
    );
    await tick();
    assert.deepEqual(writes, ['{"version":1,"edited":"local"}']);

    harness.emitValueChanged({
      key: 'web:widgets',
      value: JSON.stringify('{"version":2,"from":"external"}'),
      revision: 2,
    });
    conflictingWrite.resolve({
      status: 'error',
      error:
        'Storage(RevisionConflict { expected: StorageRevision(1), current: StorageRevision(2) })',
    });

    assert.equal(await first, false);
    await harness.store.refresh('widgets');
    assert.equal(harness.store.peek('widgets')?.draft, null);
    assert.equal(
      harness.store.peek('widgets')?.value,
      '{"version":2,"from":"external"}',
    );

    const staleRetry = await harness.store.write(
      'widgets',
      '{"version":1,"edited":"local"}',
      expectedRevision,
    );
    assert.equal(staleRetry, false);
    assert.deepEqual(writes, ['{"version":1,"edited":"local"}']);
  } finally {
    harness.dispose();
  }
});

test('a revision conflict cancels later writes queued from the stale draft', async () => {
  const conflictingWrite = deferred<StorageWriteResult>();
  const writes: string[] = [];
  let reads = 0;
  const defaultValue = { version: 1 };
  const migrate = (value: unknown) => {
    if ((value as { version?: number }).version !== 1) {
      throw new Error('unsupported storage version');
    }
    return value as typeof defaultValue;
  };
  const harness = createHarness(
    async () => {
      reads += 1;
      return {
        status: 'ok',
        data:
          reads === 1
            ? { value: '{"version":1,"from":"initial"}', revision: 1 }
            : { value: '{"version":1,"from":"external"}', revision: 2 },
      };
    },
    (_key, value) => {
      writes.push(value);
      return conflictingWrite.promise;
    },
  );
  const expectedRevision = () => {
    const entry = harness.store.peek('widgets');
    if (!entry || entry.value === undefined) return null;
    if (
      !canWriteKvStorageValue(
        entry,
        decodeKvStorageValue(entry.value, defaultValue, migrate).error,
      )
    ) {
      return null;
    }
    return harness.store.currentRevision();
  };

  try {
    harness.store.ensureRead('widgets');
    await harness.store.refresh('widgets');
    const first = harness.store.write(
      'widgets',
      '{"version":1,"from":"local-first"}',
      expectedRevision,
    );
    await tick();
    const second = harness.store.write(
      'widgets',
      '{"version":1,"from":"local-stale"}',
      expectedRevision,
    );

    harness.emitValueChanged({
      key: 'web:widgets',
      value: JSON.stringify('{"version":1,"from":"external"}'),
      revision: 2,
    });
    conflictingWrite.resolve({
      status: 'error',
      error:
        'Storage(RevisionConflict { expected: StorageRevision(1), current: StorageRevision(2) })',
    });

    assert.equal(await first, false);
    assert.equal(await second, false);
    await harness.store.refresh('widgets');
    assert.equal(
      harness.store.peek('widgets')?.value,
      '{"version":1,"from":"external"}',
    );
    assert.equal(harness.store.peek('widgets')?.draft, null);
    assert.deepEqual(writes, ['{"version":1,"from":"local-first"}']);
  } finally {
    harness.dispose();
  }
});

test('a newer external event cancels queued writes before an earlier write ack', async () => {
  const firstWrite = deferred<StorageWriteResult>();
  const writes: string[] = [];
  const harness = createHarness(
    async () => ({
      status: 'ok',
      data: { value: '"initial"', revision: 1 },
    }),
    (_key, value) => {
      writes.push(value);
      return firstWrite.promise;
    },
  );

  try {
    harness.store.ensureRead('shared');
    await harness.store.refresh('shared');
    const expectedRevision = () => harness.store.currentRevision();
    const first = harness.store.write(
      'shared',
      '"local-first"',
      expectedRevision,
    );
    await tick();
    const second = harness.store.write(
      'shared',
      '"local-stale"',
      expectedRevision,
    );

    harness.emitValueChanged({
      key: 'web:shared',
      value: JSON.stringify('"external"'),
      revision: 3,
    });
    firstWrite.resolve({ status: 'ok', data: 2 });

    assert.equal(await first, true);
    assert.equal(await second, false);
    assert.deepEqual(writes, ['"local-first"']);
    assert.equal(harness.store.peek('shared')?.value, '"external"');
    assert.equal(harness.store.peek('shared')?.revision, 3);
    assert.equal(harness.store.peek('shared')?.draft, null);
  } finally {
    harness.dispose();
  }
});

test('an active write echo does not cancel its queued successor', async () => {
  const firstWrite = deferred<StorageWriteResult>();
  const writes: Array<{ value: string; expectedRevision: number | null }> = [];
  const harness = createHarness(
    async () => ({
      status: 'ok',
      data: { value: '"initial"', revision: 1 },
    }),
    (_key, value, expectedRevision) => {
      writes.push({ value, expectedRevision });
      return writes.length === 1
        ? firstWrite.promise
        : Promise.resolve({ status: 'ok', data: 3 });
    },
  );

  try {
    harness.store.ensureRead('shared');
    await harness.store.refresh('shared');
    const expectedRevision = () => harness.store.currentRevision();
    const first = harness.store.write(
      'shared',
      '"local-first"',
      expectedRevision,
    );
    await tick();
    const second = harness.store.write(
      'shared',
      '"local-second"',
      expectedRevision,
    );

    harness.emitValueChanged({
      key: 'web:shared',
      value: JSON.stringify('"local-first"'),
      revision: 2,
    });
    firstWrite.resolve({ status: 'ok', data: 2 });

    assert.equal(await first, true);
    assert.equal(await second, true);
    assert.deepEqual(writes, [
      { value: '"local-first"', expectedRevision: 1 },
      { value: '"local-second"', expectedRevision: 2 },
    ]);
    assert.equal(harness.store.peek('shared')?.value, '"local-second"');
    assert.equal(harness.store.peek('shared')?.revision, 3);
    assert.equal(harness.store.peek('shared')?.draft, null);
  } finally {
    harness.dispose();
  }
});
