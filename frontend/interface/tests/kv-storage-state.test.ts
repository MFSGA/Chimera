import assert from 'node:assert/strict';
import test from 'node:test';
import {
  canWriteKvStorageValue,
  decodeKvStorageValue,
  EMPTY_KV_STORAGE_ENTRY,
  kvStorageEventSnapshot,
  mergeKvStorageSnapshot,
  type KvStorageEntry,
} from '../src/provider/kv-storage-state';

const entry = (patch: Partial<KvStorageEntry>): KvStorageEntry => ({
  ...EMPTY_KV_STORAGE_ENTRY,
  ...patch,
});

test('migration errors remain visible and block writes instead of replacing data', () => {
  const defaultValue = { version: 1, byInstance: {} };
  const migrationError = new Error('unsupported storage version');
  const decoded = decodeKvStorageValue(
    '{"version":2,"byInstance":{}}',
    defaultValue,
    (value) => {
      if ((value as { version?: number }).version !== 1) {
        throw migrationError;
      }
      return value as typeof defaultValue;
    },
  );

  assert.deepEqual(decoded.value, defaultValue);
  assert.equal(decoded.error, migrationError);
  const stored = {
    ...EMPTY_KV_STORAGE_ENTRY,
    value: '{"version":2,"byInstance":{}}',
    revision: 1,
    read: { status: 'done' as const, error: null },
  };
  assert.equal(canWriteKvStorageValue(stored, decoded.error), false);
  assert.equal(
    canWriteKvStorageValue(
      { ...stored, read: { status: 'done', error: migrationError } },
      null,
    ),
    false,
  );
  assert.equal(
    canWriteKvStorageValue(
      { ...stored, read: { status: 'reading', error: null } },
      null,
    ),
    false,
  );
});

test('valid migrations keep storage writable and missing values use defaults', () => {
  const defaultValue = { version: 1 };
  const decoded = decodeKvStorageValue('{"version":0}', defaultValue, () => ({
    version: 1,
  }));

  assert.deepEqual(decoded, { value: { version: 1 }, error: null });
  assert.equal(
    canWriteKvStorageValue(
      {
        ...EMPTY_KV_STORAGE_ENTRY,
        value: '{"version":1}',
        revision: 1,
        read: { status: 'done', error: null },
      },
      decoded.error,
    ),
    true,
  );
  assert.deepEqual(decodeKvStorageValue(null, defaultValue), {
    value: defaultValue,
    error: null,
  });
});

test('only a newer revision replaces the committed value', () => {
  const current = entry({ value: '"b"', revision: 5 });

  assert.equal(
    mergeKvStorageSnapshot(current, { value: '"a"', revision: 4 }),
    current,
  );
  assert.equal(
    mergeKvStorageSnapshot(current, { value: '"a"', revision: 5 }),
    current,
  );
  assert.deepEqual(
    mergeKvStorageSnapshot(current, { value: null, revision: 6 }),
    { ...current, value: null, revision: 6 },
  );
});

test('the first snapshot is accepted at revision zero', () => {
  assert.deepEqual(
    mergeKvStorageSnapshot(EMPTY_KV_STORAGE_ENTRY, {
      value: '1',
      revision: 0,
    }),
    { ...EMPTY_KV_STORAGE_ENTRY, value: '1', revision: 0 },
  );
});

test('a newer commit replaces a failed draft but not a pending one', () => {
  const snapshot = { value: '"remote"', revision: 2 };
  const pending = entry({
    revision: 1,
    draft: { id: 1, value: '"local"', failed: false },
  });
  const failed = entry({
    revision: 1,
    draft: { id: 1, value: '"local"', failed: true },
  });

  assert.equal(mergeKvStorageSnapshot(pending, snapshot).draft, pending.draft);
  assert.equal(mergeKvStorageSnapshot(failed, snapshot).draft, null);
  assert.equal(
    mergeKvStorageSnapshot(failed, { value: '"old"', revision: 1 }).draft,
    failed.draft,
  );
});

test('events decode frontend values and distinguish null from removal', () => {
  assert.deepEqual(
    kvStorageEventSnapshot({
      key: 'web:layout',
      value: JSON.stringify(JSON.stringify({ a: 1 })),
      revision: 3,
    }),
    {
      key: 'layout',
      snapshot: { value: '{"a":1}', revision: 3 },
    },
  );
  assert.deepEqual(
    kvStorageEventSnapshot({ key: 'web:css', value: '"null"', revision: 4 }),
    { key: 'css', snapshot: { value: 'null', revision: 4 } },
  );
  assert.deepEqual(
    kvStorageEventSnapshot({ key: 'web:css', value: null, revision: 5 }),
    { key: 'css', snapshot: { value: null, revision: 5 } },
  );
});

test('internal keys and malformed storage events are ignored', () => {
  assert.equal(
    kvStorageEventSnapshot({ key: 'tasks', value: '"x"', revision: 1 }),
    null,
  );
  assert.equal(
    kvStorageEventSnapshot({ key: 'web:a', value: '{', revision: 1 }),
    null,
  );
  assert.equal(
    kvStorageEventSnapshot({ key: 'web:a', value: '1', revision: 1 }),
    null,
  );
  assert.equal(
    kvStorageEventSnapshot({
      key: 'web:a',
      value: JSON.stringify('{'),
      revision: 1,
    }),
    null,
  );
});
