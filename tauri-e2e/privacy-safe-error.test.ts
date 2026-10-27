import assert from 'node:assert/strict';
import test from 'node:test';
import {
  sanitizeFrontendErrorCapture,
  sanitizeFrontendErrorMessage,
} from './privacy-safe-error.ts';

test('frontend error sanitization redacts bearer headers and serialized secrets', () => {
  const message =
    'Authorization: Bearer bearer-secret, response={"token":"token-secret","nested":{"api_key":"key-secret"}}';

  const sanitized = sanitizeFrontendErrorMessage(message);

  assert.doesNotMatch(sanitized, /bearer-secret|token-secret|key-secret/);
  assert.match(sanitized, /Authorization: \[redacted\]/);
  assert.match(sanitized, /"token":\[redacted\]/);
});

test('frontend error sanitization removes URL credentials, query secrets, and JWTs', () => {
  const message =
    'https://user:pass@example.test/?access_token=query-secret eyJhbGciOiJIUzI1NiJ9.eyJzdWIiOiIxMjM0NTY3ODkwIn0.signature-value';

  const sanitized = sanitizeFrontendErrorMessage(message);

  assert.doesNotMatch(sanitized, /user:pass|query-secret|signature-value/);
  assert.match(sanitized, /https:\/\/\[redacted\]@example\.test/);
  assert.match(sanitized, /access_token=\[redacted\]/);
  assert.match(sanitized, /\[redacted-jwt\]/);
});

test('frontend error capture is bounded and sanitizes each recorded message', () => {
  const capture = sanitizeFrontendErrorCapture({
    status: 'captured',
    events: Array.from({ length: 30 }, (_, index) => ({
      type: 'console.error',
      message: `token=secret-${index}`,
      at: index,
    })),
  });

  assert.equal(capture.events.length, 25);
  assert.equal(capture.events[0].message, 'token=[redacted]');
});
