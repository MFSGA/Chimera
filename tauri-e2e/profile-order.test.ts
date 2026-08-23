import assert from 'node:assert/strict';
import test from 'node:test';
import {
  mergeFilteredProfileOrder,
  profileOrderChanged,
} from '../frontend/chimera/src/pages/(main)/main/profiles/$type/_modules/profile-order.js';

test('profile order change detection handles unchanged and reordered lists', () => {
  assert.equal(profileOrderChanged(['a', 'b'], ['a', 'b']), false);
  assert.equal(profileOrderChanged(['a', 'b'], ['b', 'a']), true);
  assert.equal(profileOrderChanged(['a'], ['a', 'b']), true);
});

test('filtered profile reordering preserves unrelated profile positions', () => {
  assert.deepEqual(
    mergeFilteredProfileOrder(
      ['profile-a', 'script-a', 'profile-b', 'merge-a', 'profile-c'],
      ['profile-a', 'profile-b', 'profile-c'],
      ['profile-c', 'profile-a', 'profile-b'],
    ),
    ['profile-c', 'script-a', 'profile-a', 'merge-a', 'profile-b'],
  );
});

test('full profile subset can be reordered directly', () => {
  assert.deepEqual(
    mergeFilteredProfileOrder(
      ['a', 'b', 'c'],
      ['a', 'b', 'c'],
      ['c', 'b', 'a'],
    ),
    ['c', 'b', 'a'],
  );
});

test('empty filtered profile subset leaves the full order unchanged', () => {
  assert.deepEqual(mergeFilteredProfileOrder(['script-a', 'merge-a'], [], []), [
    'script-a',
    'merge-a',
  ]);
});

test('filtered profile reordering rejects changed membership', () => {
  assert.throws(
    () =>
      mergeFilteredProfileOrder(
        ['a', 'b', 'script'],
        ['a', 'b'],
        ['a', 'other'],
      ),
    /missing profile b|unexpected profile other/,
  );
});

test('filtered profile reordering rejects duplicate identifiers', () => {
  assert.throws(
    () => mergeFilteredProfileOrder(['a', 'b'], ['a', 'a'], ['a', 'a']),
    /duplicate profile identifiers/,
  );
  assert.throws(
    () => mergeFilteredProfileOrder(['a', 'a'], ['a'], ['a']),
    /duplicate profile identifiers/,
  );
});

test('filtered profile reordering rejects profiles absent from the full order', () => {
  assert.throws(
    () => mergeFilteredProfileOrder(['a', 'script'], ['a', 'b'], ['b', 'a']),
    /profile b is missing from the full order/,
  );
});

test('filtered profile reordering rejects length changes', () => {
  assert.throws(
    () => mergeFilteredProfileOrder(['a', 'b'], ['a', 'b'], ['a']),
    /length changed/,
  );
});
