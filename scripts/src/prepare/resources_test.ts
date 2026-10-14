import assert from 'node:assert/strict';
import * as path from 'jsr:@std/path';
import {
  isSidecarCacheValid,
  isVersionManagedTarget,
  resolveResource,
  resolveSidecar,
  resolveWintun,
  type ResourceResolverOptions,
} from './resources.ts';
import type { BinInfo } from './types.ts';

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

Deno.test('version-managed sidecars require a matching version stamp', () => {
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

Deno.test({
  name: 'interrupted resource download preserves the existing cache file',
  timeout: 5_000,
  fn: async () => {
    const root = await Deno.makeTempDir();
    const originalFetch = globalThis.fetch;
    const resolverContext = context(root);
    const targetPath = path.join(resolverContext.resourcesDir, 'Country.mmdb');
    await Deno.mkdir(resolverContext.resourcesDir, { recursive: true });
    await Deno.writeTextFile(targetPath, 'previous-good-resource');
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

    try {
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
      assert.equal(
        await Deno.readTextFile(targetPath),
        'previous-good-resource',
      );
      const remainingFiles = [];
      for await (const entry of Deno.readDir(resolverContext.resourcesDir)) {
        remainingFiles.push(entry.name);
      }
      assert.deepEqual(remainingFiles, ['Country.mmdb']);
    } finally {
      globalThis.fetch = originalFetch;
      await Deno.remove(root, { recursive: true });
    }
  },
});

Deno.test({
  name: 'Wintun retries discard a partial archive and preserve the installed DLL',
  timeout: 5_000,
  fn: async () => {
    const root = await Deno.makeTempDir();
    const originalFetch = globalThis.fetch;
    const resolverContext: ResourceResolverOptions = {
      ...context(root),
      platform: 'win32',
      arch: 'x64',
    };
    const targetPath = path.join(resolverContext.resourcesDir, 'wintun.dll');
    await Deno.mkdir(resolverContext.resourcesDir, { recursive: true });
    await Deno.writeTextFile(targetPath, 'previous-good-wintun');
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

    try {
      await assert.rejects(
        () => resolveWintun(resolverContext, true),
        /connection reset/,
      );
      await assert.rejects(
        () => resolveWintun(resolverContext, true),
        /wintun hash not match/,
      );
      assert.equal(fetchCalls, 2);
      assert.equal(await Deno.readTextFile(targetPath), 'previous-good-wintun');
      let tempDirExists = true;
      try {
        await Deno.stat(path.join(root, 'temp/wintun'));
      } catch (error) {
        if (!(error instanceof Deno.errors.NotFound)) throw error;
        tempDirExists = false;
      }
      assert.equal(tempDirExists, false);
    } finally {
      globalThis.fetch = originalFetch;
      await Deno.remove(root, { recursive: true });
    }
  },
});

Deno.test({
  name: 'resolveSidecar retries a missing rolling asset with refreshed metadata',
  timeout: 5_000,
  fn: async () => {
    const root = await Deno.makeTempDir();
    const originalFetch = globalThis.fetch;
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

    try {
      const result = await resolveSidecar(resolverContext, pinned);
      assert.deepEqual(calls, [
        'https://example.test/pinned-alpha',
        'https://example.test/live-alpha',
      ]);
      assert.equal(refreshCalls, 1);
      assert.equal(result.version, refreshed.version);
      assert.equal(result.cached, false);
      assert.deepEqual(
        await Deno.readFile(
          path.join(resolverContext.sidecarDir, refreshed.targetFile),
        ),
        new Uint8Array([1, 2, 3]),
      );
    } finally {
      globalThis.fetch = originalFetch;
      await Deno.remove(root, { recursive: true });
    }
  },
});

Deno.test({
  name: 'checksum failure preserves the previous sidecar and version stamp',
  timeout: 5_000,
  fn: async () => {
    const root = await Deno.makeTempDir();
    const originalFetch = globalThis.fetch;
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
    await Deno.mkdir(resolverContext.sidecarDir, { recursive: true });
    await Deno.writeTextFile(targetPath, 'previous-good-sidecar');
    await Deno.writeTextFile(versionStampPath, 'v1.2.2');
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

    try {
      await assert.rejects(
        () => resolveSidecar(resolverContext, current, { force: true }),
        /SHA-256 mismatch/,
      );
      assert.equal(
        await Deno.readTextFile(targetPath),
        'previous-good-sidecar',
      );
      assert.equal(await Deno.readTextFile(versionStampPath), 'v1.2.2');
    } finally {
      globalThis.fetch = originalFetch;
      await Deno.remove(root, { recursive: true });
    }
  },
});
