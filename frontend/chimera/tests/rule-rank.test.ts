import assert from 'node:assert/strict';
import test from 'node:test';
import { rankByValue } from '../src/pages/(main)/main/rules/_modules/rank-by-value.ts';

const entries = Array.from({ length: 200 }, (_, i) => ({
  index: i + 1,
  // Mostly zero, with ties among the rest.
  value: i % 7 === 0 ? (i * 37) % 5 : 0,
}));

test('orders as a full sort by value, then rule order', () => {
  const sorted = [...entries].sort(
    (a, b) => b.value - a.value || a.index - b.index,
  );

  assert.deepEqual(
    rankByValue(entries, (entry) => entry.value),
    sorted,
  );
});

test('leaves the entries in rule order when none has a value', () => {
  assert.deepEqual(
    rankByValue(entries, () => 0),
    entries,
  );
});
