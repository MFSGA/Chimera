import assert from 'node:assert/strict';
import { describe, it } from 'node:test';
import { subscriptionUrlSchema } from '../src/pages/(main)/main/profiles/_modules/subscription-url-schema';

describe('subscriptionUrlSchema (ref parity)', () => {
  it('accepts HTTP and HTTPS subscriptions without rewriting their paths or query', () => {
    const urls = [
      'https://example.org/clash.yaml?token=a-b',
      'http://127.0.0.1:7890/profile',
      'https://[::1]:8080/sub',
    ];
    for (const url of urls) {
      const parsed = subscriptionUrlSchema.safeParse(url);
      assert.equal(parsed.success, true, `${url} should be allowed`);
      if (parsed.success) assert.equal(parsed.data, url);
    }
  });

  it('rejects URL-parsable non-HTTP protocols before submitting a remote import', () => {
    const urls = [
      'file:///etc/passwd',
      'ftp://example.org/sub',
      'javascript:alert(1)',
      'data:text/plain,profile',
      'mailto:test@example.org',
    ];
    for (const url of urls) {
      assert.doesNotThrow(() => new URL(url));
      assert.equal(
        subscriptionUrlSchema.safeParse(url).success,
        false,
        `${url} is parseable but must not be imported`,
      );
    }
  });

  it('rejects malformed, protocol-relative, missing and non-string values', () => {
    for (const url of [
      '',
      '  ',
      'not a URL',
      '//example.org/profile',
      'http://',
      undefined,
      42,
    ]) {
      assert.equal(subscriptionUrlSchema.safeParse(url).success, false);
    }
  });
});
