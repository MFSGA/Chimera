import assert from 'node:assert/strict';
import { test } from 'node:test';
import {
  ChimeraStorage,
  dispatchLegacyStorageValueChanged,
} from '../src/services/storage';

// Contract: a namespaced backend event must notify the legacy subscriber
// registered under its original key; without prefix adaptation this assertion fails.
test('namespaced storage events notify legacy subscribers by their original key', () => {
  const key = 'connections_table_columns';
  const value = JSON.stringify([['host', true]]);
  let observed: string | null | undefined;
  const unsubscribe = ChimeraStorage.subscribe(key, (nextValue) => {
    observed = nextValue;
  });

  try {
    dispatchLegacyStorageValueChanged(`web:${key}`, JSON.stringify(value));
    assert.equal(observed, value);
  } finally {
    unsubscribe();
  }
});
