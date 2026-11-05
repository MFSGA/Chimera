import assert from 'node:assert/strict';
import { prepareRetryDelayMs } from './retry.ts';

Deno.test(
  'transient 504 and 429 download errors use bounded exponential backoff',
  () => {
    const gatewayTimeout = new Error('download failed: Gateway Timeout (504)');
    assert.deepEqual(
      [0, 1, 2, 3, 4, 5].map((attempt) =>
        prepareRetryDelayMs(gatewayTimeout, attempt),
      ),
      [2_000, 4_000, 8_000, 16_000, 16_000, 16_000],
    );
    assert.equal(
      prepareRetryDelayMs(
        new Error('download failed: Too Many Requests (429)'),
        1,
      ),
      4_000,
    );
  },
);

Deno.test('permanent errors do not incur a transient backoff', () => {
  assert.equal(
    prepareRetryDelayMs(new Error('download failed: Not Found (404)'), 2),
    0,
  );
  assert.equal(prepareRetryDelayMs(new Error('invalid archive'), 3), 0);
  assert.equal(
    prepareRetryDelayMs('download failed: Gateway Timeout (504)', 0),
    0,
  );
});
