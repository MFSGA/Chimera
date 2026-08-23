import assert from 'node:assert/strict';
import test from 'node:test';
import {
  getNextProfileUpdateTimestamp,
  getSafeProfileTimestamp,
} from '../frontend/interface/src/utils/profile-time.js';

test('profile timestamps accept only positive safe values inside the Date boundary', () => {
  assert.equal(getSafeProfileTimestamp(1_700_000_000), 1_700_000_000);
  assert.equal(getSafeProfileTimestamp(8_640_000_000_000), 8_640_000_000_000);

  for (const value of [
    undefined,
    null,
    0,
    -1,
    1.5,
    Number.NaN,
    Number.POSITIVE_INFINITY,
    Number.MAX_SAFE_INTEGER,
  ]) {
    assert.equal(getSafeProfileTimestamp(value), null);
  }
});

test('profile next-update timestamp uses integer minute intervals', () => {
  assert.equal(
    getNextProfileUpdateTimestamp(1_700_000_000, 120),
    1_700_007_200,
  );
});

test('profile next-update timestamp rejects missing and non-positive inputs', () => {
  for (const updatedAt of [undefined, null, 0, -1]) {
    assert.equal(getNextProfileUpdateTimestamp(updatedAt, 120), null);
  }
  for (const interval of [undefined, null, 0, -1]) {
    assert.equal(getNextProfileUpdateTimestamp(1_700_000_000, interval), null);
  }
});

test('profile next-update timestamp rejects fractional and non-finite inputs', () => {
  for (const value of [1.5, Number.NaN, Number.POSITIVE_INFINITY]) {
    assert.equal(getNextProfileUpdateTimestamp(value, 120), null);
    assert.equal(getNextProfileUpdateTimestamp(1_700_000_000, value), null);
  }
});

test('profile next-update timestamp rejects unsafe integer arithmetic', () => {
  assert.equal(getNextProfileUpdateTimestamp(Number.MAX_SAFE_INTEGER, 1), null);
  assert.equal(
    getNextProfileUpdateTimestamp(1_700_000_000, Number.MAX_SAFE_INTEGER),
    null,
  );
});

test('profile next-update timestamp respects the JavaScript Date boundary', () => {
  assert.equal(
    getNextProfileUpdateTimestamp(60, 143_999_999_999),
    8_640_000_000_000,
  );
  assert.equal(getNextProfileUpdateTimestamp(61, 143_999_999_999), null);
});
