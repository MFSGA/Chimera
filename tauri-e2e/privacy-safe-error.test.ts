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

test('frontend error sanitization handles compound credential names and JWE', () => {
  const message = JSON.stringify({
    client_secret: 's3cr3t',
    id_token: 'id-secret',
    sessionToken: 'session-secret',
    url: 'https://example.test/?client_secret=query-secret',
    jwe:
      'eyJhbGciOiJBMjU2S1ciLCJlbmMiOiJBMTI4R0NNIn0.' +
      'encrypted-key-value.encrypted-payload-value.initialization-vector.authentication-tag',
  });

  const sanitized = sanitizeFrontendErrorMessage(message);

  assert.doesNotMatch(
    sanitized,
    /s3cr3t|id-secret|session-secret|query-secret|encrypted-key-value|encrypted-payload-value|initialization-vector|authentication-tag/,
  );
  assert.equal((sanitized.match(/\[redacted-jwt\]/g) ?? []).length, 1);
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
