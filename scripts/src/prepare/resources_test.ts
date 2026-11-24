import assert from 'node:assert/strict';
import { mkdir, readdir, readFile, stat, writeFile } from 'node:fs/promises';
import * as path from 'node:path';
import { test } from 'node:test';
import {
  isSidecarCacheValid,
  isVersionManagedTarget,
  resolveResource,
  resolveSidecar,
  resolveWintun,
  type ResourceResolverOptions,
  type SidecarRefreshRetryPolicy,
} from './resources.ts';
import { withTempFetch } from './test_utils.ts';
import type { BinInfo } from './types.ts';

function readTextFile(file: string): Promise<string> {
  return readFile(file, 'utf8');
}

function context(root: string): ResourceResolverOptions {
  return {
    resourcesDir: path.join(root, 'resources'),
    sidecarDir: path.join(root, 'sidecars'),
    tempRoot: path.join(root, 'temp'),
    platform: 'linux',
    arch: 'x64',
  };
}

function info(overrides: Partial<BinInfo> = {}): BinInfo {
  return {
    name: 'chimera-client-alpha',
    version: '0.26.3-alpha+sha.406b0e0',
    targetFile: 'chimera-client-alpha-x86_64-unknown-linux-gnu',
    exeFile: 'chimera-client-alpha',
    tmpFile: 'chimera-client-alpha',
    downloadURL: 'https://example.test/pinned-alpha',
    ...overrides,
  };
}

test('version-managed sidecars require a matching version stamp', () => {
  assert.equal(isVersionManagedTarget('mihomo-alpha'), true);
  assert.equal(isVersionManagedTarget('chimera-client-alpha'), true);
  assert.equal(isVersionManagedTarget('chimera-service'), true);
  assert.equal(
    isSidecarCacheValid('mihomo-alpha', true, 'alpha-old', 'alpha-new'),
    false,
  );
  assert.equal(
    isSidecarCacheValid('mihomo-alpha', true, 'alpha-new', 'alpha-new'),
    true,
  );
  assert.equal(isSidecarCacheValid('mihomo', true, null, 'v1.19.32'), true);
});

test(
  'interrupted resource download preserves the existing cache file',
  { timeout: 5_000 },
  async () =>
    withTempFetch(async (root) => {
      const resolverContext = context(root);
      const targetPath = path.join(
        resolverContext.resourcesDir,
        'Country.mmdb',
      );
      await mkdir(resolverContext.resourcesDir, { recursive: true });
      await writeFile(targetPath, 'previous-good-resource');
      globalThis.fetch = (() =>
        Promise.resolve(
          new Response(
            new ReadableStream<Uint8Array>({
              start(controller) {
                controller.enqueue(new Uint8Array([1, 2, 3]));
                controller.error(new Error('connection reset'));
              },
            }),
          ),
        )) as typeof fetch;

      await assert.rejects(
        () =>
          resolveResource(
            resolverContext,
            {
              file: 'Country.mmdb',
              downloadURL: 'https://example.test/Country.mmdb',
            },
            { force: true },
          ),
        /connection reset/,
      );
      assert.equal(await readTextFile(targetPath), 'previous-good-resource');
      const remainingFiles = [];
      for await (const entry of await readdir(resolverContext.resourcesDir, {
        withFileTypes: true,
      })) {
        remainingFiles.push(entry.name);
      }
      assert.deepEqual(remainingFiles, ['Country.mmdb']);
    }),
);

test(
  'Wintun retries discard a partial archive and preserve the installed DLL',
  { timeout: 5_000 },
  async () =>
    withTempFetch(async (root) => {
      const resolverContext: ResourceResolverOptions = {
        ...context(root),
        platform: 'win32',
        arch: 'x64',
      };
      const targetPath = path.join(resolverContext.resourcesDir, 'wintun.dll');
      await mkdir(resolverContext.resourcesDir, { recursive: true });
      await writeFile(targetPath, 'previous-good-wintun');
      let fetchCalls = 0;
      globalThis.fetch = (() => {
        fetchCalls++;
        if (fetchCalls === 1) {
          return Promise.resolve(
            new Response(
              new ReadableStream<Uint8Array>({
                start(controller) {
                  controller.enqueue(new Uint8Array([1, 2, 3]));
                  controller.error(new Error('connection reset'));
                },
              }),
            ),
          );
        }
        return Promise.resolve(new Response(new Uint8Array([4, 5, 6])));
      }) as typeof fetch;

      await assert.rejects(
        () => resolveWintun(resolverContext, true),
        /connection reset/,
      );
      await assert.rejects(
        () => resolveWintun(resolverContext, true),
        /wintun hash not match/,
      );
      assert.equal(fetchCalls, 2);
      assert.equal(await readTextFile(targetPath), 'previous-good-wintun');
      let tempDirExists = true;
      try {
        await stat(path.join(root, 'temp/wintun'));
      } catch (error) {
        if ((error as NodeJS.ErrnoException).code !== 'ENOENT') throw error;
        tempDirExists = false;
      }
      assert.equal(tempDirExists, false);
    }),
);

test(
  'resolveSidecar retries a missing rolling asset with refreshed metadata',
  { timeout: 5_000 },
  async () =>
    withTempFetch(async (root) => {
      const calls: string[] = [];
      let refreshCalls = 0;
      globalThis.fetch = ((input: RequestInfo | URL) => {
        const url = String(input);
        calls.push(url);
        if (url.endsWith('pinned-alpha')) {
          return Promise.resolve(new Response('missing', { status: 404 }));
        }
        if (url.endsWith('live-alpha')) {
          return Promise.resolve(new Response(new Uint8Array([1, 2, 3])));
        }
        return Promise.reject(new Error(`unexpected fetch: ${url}`));
      }) as typeof fetch;

      const refreshed = info({
        version: '0.26.4-alpha+sha.abcdef0',
        tmpFile: 'chimera-client-alpha-live',
        downloadURL: 'https://example.test/live-alpha',
      });
      const pinned = info({
        refresh: async () => {
          refreshCalls++;
          return refreshed;
        },
      });
      const resolverContext = context(root);

      const result = await resolveSidecar(resolverContext, pinned);
      assert.deepEqual(calls, [
        'https://example.test/pinned-alpha',
        'https://example.test/live-alpha',
      ]);
      assert.equal(refreshCalls, 1);
      assert.equal(result.version, refreshed.version);
      assert.equal(result.cached, false);
      assert.deepEqual(
        await readFile(
          path.join(resolverContext.sidecarDir, refreshed.targetFile),
        ),
        Buffer.from([1, 2, 3]),
      );
    }),
);

test(
  'resolveSidecar retries rolling Alpha 404s within the configured budget',
  { timeout: 5_000 },
  async () =>
    withTempFetch(async (root) => {
      const resolverContext = context(root);
      const delays: number[] = [];
      const calls: string[] = [];
      let refreshCalls = 0;
      let liveAssetCalls = 0;
      const refreshed = info({
        version: '0.26.4-alpha+sha.abcdef0',
        targetFile: 'chimera-client-alpha-x86_64-unknown-linux-gnu',
        tmpFile: 'chimera-client-alpha-live',
        downloadURL: 'https://example.test/live-alpha',
      });
      const pinned = info({
        refresh: async () => {
          refreshCalls++;
          if (refreshCalls === 1) {
            throw new Error('GitHub API error: Not Found (404)');
          }
          return refreshed;
        },
      });
      const retryPolicy: SidecarRefreshRetryPolicy = {
        delaysMs: [10, 20],
        shouldRetry: (error) =>
          error instanceof Error && error.message.includes('(404)'),
        sleep: async (delayMs) => {
          delays.push(delayMs);
        },
      };
      globalThis.fetch = ((input: RequestInfo | URL) => {
        const url = String(input);
        calls.push(url);
        if (url.endsWith('pinned-alpha')) {
          return Promise.resolve(new Response('missing', { status: 404 }));
        }
        if (url.endsWith('live-alpha')) {
          liveAssetCalls++;
          return Promise.resolve(
            liveAssetCalls === 1
              ? new Response('missing', { status: 404 })
              : new Response(new Uint8Array([1, 2, 3])),
          );
        }
        return Promise.reject(new Error(`unexpected fetch: ${url}`));
      }) as typeof fetch;

      const result = await resolveSidecar(
        resolverContext,
        pinned,
        { force: true },
        retryPolicy,
      );
      assert.deepEqual(delays, [10, 20]);
      assert.equal(refreshCalls, 3);
      assert.deepEqual(calls, [
        'https://example.test/pinned-alpha',
        'https://example.test/live-alpha',
        'https://example.test/live-alpha',
      ]);
      assert.equal(result.version, refreshed.version);
      assert.equal(result.cached, false);
      assert.deepEqual(
        await readFile(
          path.join(resolverContext.sidecarDir, refreshed.targetFile),
        ),
        Buffer.from([1, 2, 3]),
      );
      assert.equal(
        await readTextFile(
          `${path.join(resolverContext.sidecarDir, refreshed.targetFile)}.version`,
        ),
        refreshed.version,
      );
    }),
);

test(
  'resolveSidecar preserves the original 404 when refresh fails with 503',
  { timeout: 5_000 },
  async () =>
    withTempFetch(async (root) => {
      let refreshCalls = 0;
      const delays: number[] = [];
      globalThis.fetch = (() =>
        Promise.resolve(
          new Response('missing', { status: 404 }),
        )) as typeof fetch;

      await assert.rejects(
        () =>
          resolveSidecar(
            context(root),
            info({
              refresh: async () => {
                refreshCalls++;
                throw new Error('GitHub API error: Service Unavailable (503)');
              },
            }),
            { force: true },
            {
              delaysMs: [10],
              shouldRetry: (error) =>
                error instanceof Error && error.message.includes('(404)'),
              sleep: async (delayMs) => {
                delays.push(delayMs);
              },
            },
          ),
        /\(404\)/,
      );
      assert.equal(refreshCalls, 1);
      assert.deepEqual(delays, []);
    }),
);

test(
  'resolveSidecar stops after the configured rolling Alpha 404 budget',
  { timeout: 5_000 },
  async () =>
    withTempFetch(async (root) => {
      const resolverContext = context(root);
      const delays: number[] = [];
      let refreshCalls = 0;
      let assetCalls = 0;
      const refreshed = info({
        version: '0.26.4-alpha+sha.abcdef0',
        targetFile: 'chimera-client-alpha-x86_64-unknown-linux-gnu',
        tmpFile: 'chimera-client-alpha-live',
        downloadURL: 'https://example.test/live-alpha',
      });
      globalThis.fetch = (() => {
        assetCalls++;
        return Promise.resolve(new Response('missing', { status: 404 }));
      }) as typeof fetch;

      const oldTargetPath = path.join(
        resolverContext.sidecarDir,
        info().targetFile,
      );
      await mkdir(resolverContext.sidecarDir, { recursive: true });
      await writeFile(oldTargetPath, 'previous-good-sidecar');
      await writeFile(`${oldTargetPath}.version`, '0.26.2-alpha+sha.old');

      await assert.rejects(
        () =>
          resolveSidecar(
            resolverContext,
            info({
              refresh: async () => {
                refreshCalls++;
                return refreshed;
              },
            }),
            { force: true },
            {
              delaysMs: [10, 20],
              shouldRetry: (error) =>
                error instanceof Error && error.message.includes('(404)'),
              sleep: async (delayMs) => {
                delays.push(delayMs);
              },
            },
          ),
        /\(404\)/,
      );
      assert.deepEqual(delays, [10, 20]);
      assert.equal(refreshCalls, 3);
      assert.equal(assetCalls, 4);
      assert.equal(await readTextFile(oldTargetPath), 'previous-good-sidecar');
      assert.equal(
        await readTextFile(`${oldTargetPath}.version`),
        '0.26.2-alpha+sha.old',
      );
    }),
);

test(
  'checksum failure preserves the previous sidecar and version stamp',
  { timeout: 5_000 },
  async () =>
    withTempFetch(async (root) => {
      const resolverContext = context(root);
      const current = info({
        name: 'chimera-service',
        version: 'v1.2.3',
        targetFile: 'chimera-service-x86_64-unknown-linux-gnu',
        tmpFile: 'chimera-service.tar.gz',
        checksumURL: 'https://example.test/service.sha256',
        downloadURL: 'https://example.test/service.tar.gz',
      });
      const targetPath = path.join(
        resolverContext.sidecarDir,
        current.targetFile,
      );
      const versionStampPath = `${targetPath}.version`;
      await mkdir(resolverContext.sidecarDir, { recursive: true });
      await writeFile(targetPath, 'previous-good-sidecar');
      await writeFile(versionStampPath, 'v1.2.2');
      globalThis.fetch = ((input: RequestInfo | URL) => {
        const url = String(input);
        if (url.endsWith('service.tar.gz')) {
          return Promise.resolve(new Response(new Uint8Array([4, 5, 6])));
        }
        if (url.endsWith('service.sha256')) {
          return Promise.resolve(
            new Response(`${'0'.repeat(64)}  service.tar.gz\n`),
          );
        }
        return Promise.reject(new Error(`unexpected fetch: ${url}`));
      }) as typeof fetch;

      await assert.rejects(
        () => resolveSidecar(resolverContext, current, { force: true }),
        /SHA-256 mismatch/,
      );
      assert.equal(await readTextFile(targetPath), 'previous-good-sidecar');
      assert.equal(await readTextFile(versionStampPath), 'v1.2.2');
    }),
);
