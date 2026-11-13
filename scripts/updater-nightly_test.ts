import assert from 'node:assert/strict';
import { execFile } from 'node:child_process';
import { mkdtemp, rm } from 'node:fs/promises';
import { tmpdir } from 'node:os';
import * as path from 'node:path';
import { test } from 'node:test';
import { promisify } from 'node:util';
import type { Octokit } from 'octokit';
import {
  createNightlyUpdaterPayloads,
  getOrCreateUpdaterRelease,
  nightlyBuildHash,
  resolveShortHash,
  resolveUpdater,
} from './updater-nightly.ts';

const exec = promisify(execFile);

function assets(names: string[]) {
  return names.map((name, index) => ({
    id: index + 1,
    name,
    browser_download_url: `https://github.com/MFSGA/Chimera/releases/download/pre-release/${name}`,
  }));
}

const stableNames = [
  'Chimera_0.27.1_x64-setup.exe',
  'Chimera_0.27.1_arm64-setup.exe',
  'Chimera.x64.app.tar.gz',
  'Chimera.aarch64.app.tar.gz',
  'Chimera_0.27.1_amd64.AppImage',
];
const fixedNames = ['Chimera_0.27.1_x64-fixed-webview-setup.exe'];

function paired(names: string[]) {
  return assets(names.flatMap((name) => [name, `${name}.sig`]));
}

test('a Node import does not start GitHub updater mutations', () => {
  assert.equal(typeof resolveUpdater, 'function');
  assert.equal(nightlyBuildHash('v0.15.3-alpha+abcdef0123456789'), 'abcdef0');
  assert.equal(nightlyBuildHash('0.15.3+abcdef0'), 'abcdef0');
  assert.equal(nightlyBuildHash('v0.15.3'), undefined);
  assert.equal(nightlyBuildHash('v0.15.3-alpha+bad'), undefined);
});

test('nightly short commit uses latest.json first, with strict version checks', async () => {
  const original = globalThis.fetch;
  try {
    globalThis.fetch = (async () =>
      Response.json({ version: 'v0.15.3-alpha+abcdef012345' })) as typeof fetch;
    assert.equal(await resolveShortHash(assets(['latest.json'])), 'abcdef0');
  } finally {
    globalThis.fetch = original;
  }
});

test('missing latest.json hash falls back to exact seven chars of the pre-release Git tag', async () => {
  const root = await mkdtemp(path.join(tmpdir(), 'chimera-updater-tag-'));
  const original = globalThis.fetch;
  try {
    await exec('git', ['init', '-q'], { cwd: root });
    await exec(
      'git',
      [
        '-c',
        'user.name=Fixture',
        '-c',
        'user.email=fixture@example.invalid',
        'commit',
        '--allow-empty',
        '-qm',
        'tag-fixture',
      ],
      { cwd: root },
    );
    await exec('git', ['tag', 'pre-release'], { cwd: root });
    const { stdout } = await exec('git', ['rev-parse', 'HEAD'], { cwd: root });
    globalThis.fetch = (async () =>
      Response.json({ version: 'invalid' })) as typeof fetch;
    assert.equal(
      await resolveShortHash(assets(['latest.json']), root),
      stdout.trim().slice(0, 7),
    );
  } finally {
    globalThis.fetch = original;
    await rm(root, { recursive: true, force: true });
  }
});

test('standard updater builds all platform entries and valid proxied URLs', async () => {
  const input = paired(stableNames);
  const signatures: string[] = [];
  const { updateData, proxyData } = await createNightlyUpdaterPayloads(
    input,
    '0.15.3',
    'abcdef0',
    false,
    async (url) => {
      signatures.push(url);
      return 'unit-test-signature';
    },
    () => '2026-11-12T04:00:00.000Z',
  );
  assert.equal(signatures.length, 5);
  assert.equal(updateData.version, 'v0.15.3-alpha+abcdef0');
  assert.equal(updateData.pub_date, '2026-11-12T04:00:00.000Z');
  for (const target of [
    'windows-x86_64',
    'windows-aarch64',
    'linux',
    'darwin',
    'darwin-aarch64',
  ]) {
    assert.equal(updateData.platforms[target].signature, 'unit-test-signature');
    assert.ok(
      updateData.platforms[target].url.startsWith('https://github.com/MFSGA/'),
    );
    assert.ok(
      proxyData.platforms[target].url.startsWith('https://gh-proxy.com/'),
    );
  }
});

test('fixed WebView updater accepts only fixed assets and fails on missing signatures', async () => {
  const selected = paired([...stableNames, ...fixedNames]);
  const result = await createNightlyUpdaterPayloads(
    selected,
    '0.15.3',
    'abcdef0',
    true,
    async () => 'signed',
  );
  assert.ok(result.updateData.platforms['windows-x86_64']);
  assert.equal(result.updateData.platforms['darwin'], undefined);
  await assert.rejects(
    createNightlyUpdaterPayloads(
      assets(fixedNames),
      '0.15.3',
      'abcdef0',
      true,
      async () => 'signed',
    ),
    /missing updater signature/,
  );
});

test('missing targets and duplicate updater assets cause failure before publishing', async () => {
  await assert.rejects(
    createNightlyUpdaterPayloads(
      paired(['Chimera_0.27.1_x64-setup.exe']),
      '0.15.3',
      'abcdef0',
      false,
      async () => 'signed',
    ),
    /missing updater platform/,
  );
  await assert.rejects(
    createNightlyUpdaterPayloads(
      paired([...fixedNames, ...fixedNames]),
      '0.15.3',
      'abcdef0',
      true,
      async () => 'signed',
    ),
    /multiple updater assets/,
  );
});

test('a missing updater tag allows creating a replacement release', async () => {
  let created = 0;
  const fake = {
    rest: {
      repos: {
        getReleaseByTag: async () => {
          throw Object.assign(new Error('missing'), { status: 404 });
        },
        createRelease: async () => {
          created++;
          return { data: { id: 9, assets: [] } };
        },
      },
    },
  } as unknown as Octokit;
  const release = await getOrCreateUpdaterRelease(fake, {
    owner: 'MFSGA',
    repo: 'Chimera',
  });
  assert.equal(release.id, 9);
  assert.equal(created, 1);
});

test('release lookup errors other than 404 must not create a new release', async () => {
  let creates = 0;
  const fake = {
    rest: {
      repos: {
        getReleaseByTag: async () => {
          throw Object.assign(new Error('forbidden'), { status: 403 });
        },
        createRelease: async () => {
          creates++;
          return { data: { id: 2, assets: [] } };
        },
      },
    },
  } as unknown as Octokit;
  await assert.rejects(
    getOrCreateUpdaterRelease(fake, { owner: 'MFSGA', repo: 'Chimera' }),
    /forbidden/,
  );
  assert.equal(creates, 0);
});
