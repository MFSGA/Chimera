import assert from 'node:assert/strict';
import { createBinaryResolvers } from './binaries.ts';
import { createPrepareTasks, selectPrepareTasks } from './tasks.ts';
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
