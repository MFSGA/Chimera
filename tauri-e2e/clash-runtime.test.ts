import assert from 'node:assert/strict';
import test from 'node:test';
import { readClashRuntimeConfig } from './clash-runtime.ts';

test('re-reads the effective controller while the runtime is starting', async () => {
  const originalBrowser = (globalThis as Record<string, unknown>).browser;
  const originalFetch = globalThis.fetch;
  const servers = ['127.0.0.1:59180', '127.0.0.1:59181'];
  const requestedUrls: string[] = [];
  let infoCalls = 0;

  Object.defineProperty(globalThis, 'browser', {
    configurable: true,
    value: {
      execute: async () => ({
        secret: 'chimera',
        server: servers[Math.min(infoCalls++, servers.length - 1)],
      }),
    },
    writable: true,
  });
  globalThis.fetch = (async (input, init) => {
    requestedUrls.push(String(input));
    assert.ok(init?.signal instanceof AbortSignal);

    if (requestedUrls.length === 1) {
      throw new Error('connection refused');
    }

    return new Response(JSON.stringify({ ipv6: false }), {
      headers: { 'content-type': 'application/json' },
    });
  }) as typeof fetch;

  try {
    const config = await readClashRuntimeConfig<{ ipv6: boolean }>();

    assert.deepEqual(config, { ipv6: false });
    assert.deepEqual(requestedUrls, [
      'http://127.0.0.1:59180/configs',
      'http://127.0.0.1:59181/configs',
    ]);
    assert.equal(infoCalls, 2);
  } finally {
    globalThis.fetch = originalFetch;
    if (originalBrowser === undefined) {
      Reflect.deleteProperty(globalThis, 'browser');
    } else {
      Object.defineProperty(globalThis, 'browser', {
        configurable: true,
        value: originalBrowser,
        writable: true,
      });
    }
  }
});
