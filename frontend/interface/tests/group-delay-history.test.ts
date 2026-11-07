import assert from 'node:assert/strict';
import test from 'node:test';
import { mergeGroupDelayHistory } from '../src/ipc/use-clash-proxies.js';

test('partial group delay updates only reported members and preserves omitted history', () => {
  const existing = [
    { name: 'DIRECT', history: [{ time: 'old', delay: 17 }] },
    { name: 'REJECT', history: [{ time: 'old', delay: 23 }] },
  ];
  const updated = mergeGroupDelayHistory(existing, { DIRECT: 0 });
  assert.deepEqual(
    updated[0].history.map((item) => item.delay),
    [17, 0],
  );
  assert.equal(updated[1], existing[1]);
  assert.deepEqual(existing[0].history, [{ time: 'old', delay: 17 }]);
  assert.deepEqual(mergeGroupDelayHistory(existing, {}), existing);
});
