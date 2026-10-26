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

    harness.emitResync();
    await tick();
    assert.equal(reads, 2);
    assert.equal(harness.store.peek('layout')?.value, '{"source":"resync"}');
    assert.equal(harness.store.peek('layout')?.revision, 7);
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
