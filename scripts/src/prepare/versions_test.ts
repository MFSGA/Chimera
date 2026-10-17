import { assertEquals } from 'jsr:@std/assert@1';
import {
  extractVersionFromAssetList,
  fetchText,
  isValidVersion,
  normalizeVersion,
} from './versions.ts';

Deno.test('normalizes and validates versions', () => {
  assertEquals(normalizeVersion(), undefined);
  assertEquals(normalizeVersion('Not Found'), undefined);
  assertEquals(normalizeVersion('v1.2.3'), 'v1.2.3');
  assertEquals(isValidVersion('v1.2.3-rc.1+build'), true);
  assertEquals(isValidVersion('bad/version'), false);
});

Deno.test('extracts a version matching an asset template', () => {
  assertEquals(
    extractVersionFromAssetList(
      'mihomo-linux-amd64-v1.19.0-alpha.2.gz other asset',
      'mihomo-linux-amd64-{}.gz',
    ),
    'v1.19.0-alpha.2',
  );
  assertEquals(
    extractVersionFromAssetList(
      'mihomo-linux-amd64-latest.gz',
      'no-placeholder',
    ),
    undefined,
  );
  assertEquals(
    extractVersionFromAssetList(
      'mihomo-linux-amd64-bad/version.gz',
      'mihomo-linux-amd64-{}.gz',
    ),
    undefined,
  );
});

Deno.test(
  'fetchText preserves caller headers alongside the default user agent',
  async () => {
    const originalFetch = globalThis.fetch;
    let captured = new Headers();
    globalThis.fetch = (_input, init) => {
      captured = new Headers(init?.headers);
      return Promise.resolve(new Response('release page'));
    };

    try {
      assertEquals(
        await fetchText('https://example.invalid/releases', {
          headers: new Headers({ Authorization: 'Bearer test-token' }),
        }),
        'release page',
      );
      assertEquals(captured.get('Authorization'), 'Bearer test-token');
      assertEquals(
        captured.get('User-Agent'),
        'Mozilla/5.0 (Windows NT 10.0; Win64; x64; rv:131.0) Gecko/20100101 Firefox/131.0',
      );
    } finally {
      globalThis.fetch = originalFetch;
    }
  },
);
