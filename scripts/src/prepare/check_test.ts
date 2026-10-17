import assert from 'node:assert/strict';
import { createBinaryResolvers } from './binaries.ts';
import type { SidecarRefreshRetryPolicy } from './resources.ts';
import { createPrepareTasks, selectPrepareTasks } from './tasks.ts';
import { withTempFetch } from './test_utils.ts';
import type { VersionManifest } from './types.ts';

const versionManifest = {
  latest: {
    mihomo: 'mihomo-version',
    mihomo_alpha: 'mihomo-alpha-version',
    clash_rs: 'clash-rs-version',
    chimera_client: 'chimera-client-version',
    chimera_client_alpha: 'chimera-client-alpha-version',
    clash_premium: 'clash-version',
    clash_rs_alpha: 'clash-rs-alpha-version',
  },
  arch_template: {},
} as VersionManifest;

function createTasks(platform: string) {
  const binary = createBinaryResolvers({
    versionManifest,
    sidecarHost: 'x86_64-unknown-linux-gnu',
    platform,
    arch: 'x64',
    workspaceRoot: '/workspace',
  });

  return createPrepareTasks({
    versionManifest,
    binary,
    resourceContext: {
      resourcesDir: '/workspace/resources',
      sidecarDir: '/workspace/sidecar',
      tempRoot: '/workspace/temp',
      platform,
      arch: 'x64',
    },
  });
}

Deno.test(
  'prepare task registry includes supported cores and resources',
  () => {
    const tasks = createTasks('linux');

    assert.deepEqual(
      tasks.map((task) => task.name),
      [
        'clash',
        'mihomo',
        'mihomo-alpha',
        'clash-rs',
        'clash-rs-alpha',
        'chimera-client',
        'chimera-client-alpha',
        'wintun',
        'chimera-service',
        'mmdb',
        'geoip',
        'geosite',
        'enableLoopback',
      ],
    );
    assert(tasks.every((task) => task.retry === 5));
    assert.deepEqual(
      tasks.filter((task) => task.winOnly).map((task) => task.name),
      ['wintun', 'enableLoopback'],
    );
  },
);

Deno.test(
  'prepare task selection excludes Windows resources on other platforms',
  () => {
    const tasks = createTasks('darwin');

    assert.deepEqual(
      selectPrepareTasks(tasks, 'darwin').map((task) => task.name),
      tasks.filter((task) => !task.winOnly).map((task) => task.name),
    );
    assert.equal(selectPrepareTasks(tasks, 'win32').length, tasks.length);
  },
);

Deno.test({
  name: 'rolling Alpha retry policy is wired only to the Chimera Client task',
  timeout: 5_000,
  fn: async () =>
    withTempFetch(async (root) => {
      let alphaRequests = 0;
      let alphaRefreshes = 0;
      let mihomoAlphaRefreshes = 0;
      const delays: number[] = [];
      globalThis.fetch = ((input: RequestInfo | URL) => {
        const url = String(input);
        if (url.includes('/chimera-client-alpha/')) {
          alphaRequests++;
          return Promise.resolve(
            alphaRequests < 3
              ? new Response('rolling asset unavailable', { status: 404 })
              : new Response(new Uint8Array([1, 2, 3])),
          );
        }
        if (url.includes('/mihomo-alpha/')) {
          return Promise.resolve(
            new Response('asset unavailable', { status: 404 }),
          );
        }
        return Promise.reject(new Error(`unexpected fetch: ${url}`));
      }) as typeof fetch;

      const binary = createBinaryResolvers({
        versionManifest,
        sidecarHost: 'x86_64-unknown-linux-gnu',
        platform: 'linux',
        arch: 'x64',
        workspaceRoot: root,
      });
      const latestClientAlphaInfo = {
        name: 'chimera-client-alpha',
        version: '0.26.4-alpha+sha.abcdef0',
        targetFile: 'chimera-client-alpha',
        exeFile: 'chimera-client-alpha',
        tmpFile: 'chimera-client-alpha',
        downloadURL: 'https://example.test/chimera-client-alpha/latest',
      };
      binary.chimeraClientAlpha = () => ({
        name: 'chimera-client-alpha',
        version: '0.26.3-alpha+sha.406b0e0',
        targetFile: 'chimera-client-alpha',
        exeFile: 'chimera-client-alpha',
        tmpFile: 'chimera-client-alpha',
        downloadURL: 'https://example.test/chimera-client-alpha/pinned',
        refresh: async () => {
          alphaRefreshes++;
          return latestClientAlphaInfo;
        },
      });
      binary.mihomoAlpha = async () => ({
        name: 'mihomo-alpha',
        version: 'alpha-test',
        targetFile: 'mihomo-alpha',
        exeFile: 'mihomo-alpha',
        tmpFile: 'mihomo-alpha',
        downloadURL: 'https://example.test/mihomo-alpha/pinned',
        refresh: async () => {
          mihomoAlphaRefreshes++;
          return {
            name: 'mihomo-alpha',
            version: 'alpha-test-latest',
            targetFile: 'mihomo-alpha',
            exeFile: 'mihomo-alpha',
            tmpFile: 'mihomo-alpha',
            downloadURL: 'https://example.test/mihomo-alpha/latest',
          };
        },
      });

      const retryPolicy: SidecarRefreshRetryPolicy = {
        delaysMs: [0],
        shouldRetry: (error) =>
          error instanceof Error && error.message.includes('(404)'),
        sleep: async (delayMs) => {
          delays.push(delayMs);
        },
      };
      const tasks = createPrepareTasks({
        force: true,
        versionManifest,
        binary,
        resourceContext: {
          resourcesDir: `${root}/resources`,
          sidecarDir: `${root}/sidecars`,
          tempRoot: `${root}/temp`,
          platform: 'linux',
          arch: 'x64',
        },
        chimeraClientAlphaRefreshRetryPolicy: retryPolicy,
      });

      await tasks.find((task) => task.name === 'chimera-client-alpha')!.func();
      assert.equal(alphaRequests, 3);
      assert.equal(alphaRefreshes, 2);
      assert.deepEqual(delays, [0]);

      await assert.rejects(
        () => tasks.find((task) => task.name === 'mihomo-alpha')!.func(),
        /\(404\)/,
      );
      assert.equal(mihomoAlphaRefreshes, 1);
      assert.deepEqual(delays, [0]);
    }),
});
