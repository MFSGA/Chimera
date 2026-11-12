import assert from 'node:assert/strict';
import test from 'node:test';
import {
  changesOf,
  retainKnownDraft,
  sameOrder,
  serverChainOf,
} from '../src/pages/(main)/main/profiles/_modules/transform-chain-model';

test('server chain keeps persisted order and removes unknown transform UIDs', () => {
  assert.deepEqual(serverChainOf(['b', 'missing', 'a'], ['a', 'b', 'c']), [
    'b',
    'a',
  ]);
});

test('candidate refresh removes deleted UIDs without auto-adding new candidates', () => {
  assert.deepEqual(
    retainKnownDraft(['c', 'a', 'deleted'], ['a', 'b', 'c', 'new']),
    ['c', 'a'],
  );
});

test('change summary distinguishes additions, removals, and reordering', () => {
  assert.deepEqual(changesOf(['c', 'a', 'new'], ['a', 'b', 'c']), {
    added: 1,
    removed: 1,
    reordered: true,
  });
});

test('membership-only edits do not report a reorder', () => {
  assert.deepEqual(changesOf(['a', 'new', 'b'], ['a', 'b', 'removed']), {
    added: 1,
    removed: 1,
    reordered: false,
  });
});

test('sameOrder compares both membership and position', () => {
  assert.equal(sameOrder(['a', 'b'], ['a', 'b']), true);
  assert.equal(sameOrder(['a', 'b'], ['b', 'a']), false);
  assert.equal(sameOrder(['a'], ['a', 'b']), false);
});
