import assert from 'node:assert/strict';
import {
  isSidecarCacheValid,
  isVersionManagedTarget,
  resolveSidecar,
} from './resources.ts';
import type { BinInfo } from './types.ts';

function info(overrides: Partial<BinInfo> = {}): BinInfo {
  return {
    name: 'chimera-client-alpha',
    version: '0.26.3-alpha+sha.406b0e0',
    targetFile: 'chimera-client-alpha-aarch64-apple-darwin',
    exeFile: 'chimera-client-0.26.3-aarch64-apple-darwin/clash-rs',
    tmpFile: 'chimera-client-0.26.3-aarch64-apple-darwin.tar.gz',
    downloadURL: 'https://example.invalid/old.tar.gz',
    ...overrides,
  };
}

Deno.test(
  'a Mihomo Alpha cache is invalidated when its version changes',
  () => {
    assert.equal(isVersionManagedTarget('mihomo-alpha'), true);
    assert.equal(
      isSidecarCacheValid(
        'mihomo-alpha',
        true,
        'alpha-1234567',
        'alpha-7654321',
      ),
      false,
    );
    assert.equal(
      isSidecarCacheValid(
        'mihomo-alpha',
        true,
        'alpha-7654321',
        'alpha-7654321',
      ),
      true,
    );
    assert.equal(
      isSidecarCacheValid('mihomo-alpha', true, null, 'alpha-7654321'),
      false,
    );
    assert.equal(isSidecarCacheValid('mihomo', true, null, 'v1.19.32'), true);
  },
);

Deno.test(
  'resolveSidecar retries once with refreshed release metadata',
  async () => {
    const refreshedInfo = info({
      version: '0.26.4-alpha+sha.abcdef0',
      tmpFile: 'chimera-client-0.26.4-aarch64-apple-darwin.tar.gz',
      downloadURL: 'https://example.invalid/new.tar.gz',
    });
    const calls: string[] = [];
    let refreshCalls = 0;
    const result = await resolveSidecar(
      info({
        refresh: async () => {
          refreshCalls++;
          return refreshedInfo;
        },
      }),
      async (binary) => {
        calls.push(binary.downloadURL);
        if (calls.length === 1) throw new Error('old rolling asset is missing');
        return binary.version;
      },
    );

    assert.deepEqual(calls, [
      'https://example.invalid/old.tar.gz',
      'https://example.invalid/new.tar.gz',
    ]);
    assert.equal(refreshCalls, 1);
    assert.equal(result, refreshedInfo.version);
  },
);

Deno.test(
  'resolveSidecar preserves the original error when refresh fails',
  async () => {
    const originalError = new Error('pinned asset returned 404');
    const debug: string[] = [];
    await assert.rejects(
      () =>
        resolveSidecar(
          info({
            refresh: async () => {
              throw new Error('GitHub API unavailable');
            },
          }),
          async () => {
            throw originalError;
          },
          (message) => debug.push(message),
        ),
      (error) => error === originalError,
    );

    assert.equal(debug.length, 1);
    assert.match(debug[0], /^failed to refresh chimera-client-alpha:/);
  },
);
