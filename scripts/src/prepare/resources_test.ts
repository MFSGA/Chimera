import assert from 'node:assert/strict';
import { resolveSidecar, type SidecarRefreshRetryPolicy } from './resources.ts';
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

Deno.test(
  'resolveSidecar retries a missing rolling release until its tag and asset return',
  async () => {
    const delays: number[] = [];
    const materialized: string[] = [];
    let refreshCalls = 0;
    const refreshedInfo = info({
      version: '0.27.0-alpha+sha.abcdef0',
      tmpFile: 'chimera-client-0.27.0-aarch64-apple-darwin.tar.gz',
      downloadURL: 'https://example.invalid/live.tar.gz',
    });
    const refreshRetry: SidecarRefreshRetryPolicy = {
      delaysMs: [10, 20],
      shouldRetry: (error) =>
        error instanceof Error && error.message.includes('(404)'),
      sleep: async (delayMs) => {
        delays.push(delayMs);
      },
    };

    const result = await resolveSidecar(
      info({
        refresh: async () => {
          refreshCalls++;
          if (refreshCalls === 1) {
            throw new Error('GitHub API error: Not Found (404)');
          }
          return refreshedInfo;
        },
      }),
      async (binary) => {
        materialized.push(binary.downloadURL);
        if (
          binary.downloadURL !== refreshedInfo.downloadURL ||
          materialized.filter((url) => url === refreshedInfo.downloadURL)
            .length === 1
        ) {
          throw new Error('download failed: Not Found (404)');
        }
        return binary.version;
      },
      () => {},
      refreshRetry,
    );

    assert.deepEqual(delays, [10, 20]);
    assert.equal(refreshCalls, 3);
    assert.deepEqual(materialized, [
      'https://example.invalid/old.tar.gz',
      'https://example.invalid/live.tar.gz',
      'https://example.invalid/live.tar.gz',
    ]);
    assert.equal(result, refreshedInfo.version);
  },
);

Deno.test('resolveSidecar does not retry non-404 refresh errors', async () => {
  const delays: number[] = [];
  let refreshCalls = 0;
  const originalError = new Error('download failed: Not Found (404)');
  await assert.rejects(
    () =>
      resolveSidecar(
        info({
          refresh: async () => {
            refreshCalls++;
            throw new Error('GitHub API error: Service Unavailable (503)');
          },
        }),
        async () => {
          throw originalError;
        },
        () => {},
        {
          delaysMs: [10],
          shouldRetry: (error) =>
            error instanceof Error && error.message.includes('(404)'),
          sleep: async (delayMs) => {
            delays.push(delayMs);
          },
        },
      ),
    (error) => error === originalError,
  );

  assert.equal(refreshCalls, 1);
  assert.deepEqual(delays, []);
});

Deno.test(
  'resolveSidecar stops after the configured 404 retry budget',
  async () => {
    const delays: number[] = [];
    const materialized: string[] = [];
    let refreshCalls = 0;
    const refreshedInfo = info({
      version: '0.27.0-alpha+sha.abcdef0',
      downloadURL: 'https://example.invalid/live.tar.gz',
    });

    await assert.rejects(
      () =>
        resolveSidecar(
          info({
            refresh: async () => {
              refreshCalls++;
              return refreshedInfo;
            },
          }),
          async (binary) => {
            materialized.push(binary.downloadURL);
            throw new Error(
              `asset attempt ${materialized.length} Not Found (404)`,
            );
          },
          () => {},
          {
            delaysMs: [10, 20],
            shouldRetry: (error) =>
              error instanceof Error && error.message.includes('(404)'),
            sleep: async (delayMs) => {
              delays.push(delayMs);
            },
          },
        ),
      /asset attempt 4 Not Found \(404\)/,
    );

    assert.deepEqual(delays, [10, 20]);
    assert.equal(refreshCalls, 3);
    assert.deepEqual(materialized, [
      'https://example.invalid/old.tar.gz',
      'https://example.invalid/live.tar.gz',
      'https://example.invalid/live.tar.gz',
      'https://example.invalid/live.tar.gz',
    ]);
  },
);
