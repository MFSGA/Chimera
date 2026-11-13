import assert from 'node:assert/strict';
import { mkdir, mkdtemp, readFile, rm, writeFile } from 'node:fs/promises';
import { tmpdir } from 'node:os';
import * as path from 'node:path';
import { test } from 'node:test';
import {
  buildVersionManifest,
  generateLatestVersion,
} from './generate-latest-version.ts';
import { resolveChimeraClient, resolveChimeraClientAlpha } from './manifest.ts';
import type { ArchMapping } from './src/prepare/types.ts';

const names = [
  'mihomo',
  'mihomo_alpha',
  'clash_rs',
  'chimera_client',
  'chimera_client_alpha',
  'clash_premium',
  'clash_rs_alpha',
] as const;

function fixtureResolvers() {
  return names.map((name, index) => async () => ({
    name,
    version: `v1.${index}.0`,
    archMapping: { 'windows-x86_64': `${name}-{}.zip` } as ArchMapping,
  }));
}

test('Node generator preserves the seven Chimera core names and architecture mappings', async () => {
  const results = await Promise.all(
    fixtureResolvers().map((resolve) => resolve()),
  );
  const manifest = buildVersionManifest(results);
  assert.equal(manifest.manifest_version, 1);
  assert.deepEqual(Object.keys(manifest.latest), [...names]);
  assert.deepEqual(Object.keys(manifest.arch_template), [...names]);
  assert.equal(manifest.latest.chimera_client_alpha, 'v1.4.0');
  assert.equal(
    manifest.arch_template.chimera_client['windows-x86_64'],
    'chimera_client-{}.zip',
  );
});

test('Node generator writes once and skips identical content without touching timestamp', async () => {
  const root = await mkdtemp(path.join(tmpdir(), 'chimera-manifest-node-'));
  try {
    const options = {
      workspaceRoot: root,
      resolve: fixtureResolvers(),
      now: () => '2026-11-12T00:00:00.000Z',
    };
    const first = await generateLatestVersion(options);
    assert.equal(first.changed, true);
    const file = path.join(root, 'manifest/version.json');
    const before = await readFile(file, 'utf8');
    const saved = JSON.parse(before);
    assert.equal(saved.updated_at, '2026-11-12T00:00:00.000Z');
    assert.deepEqual(Object.keys(saved.latest), [...names]);
    assert.equal(before, JSON.stringify(saved, null, 2) + '\n');

    const again = await generateLatestVersion({
      ...options,
      now: () => {
        throw new Error('timestamp must not change');
      },
    });
    assert.equal(again.changed, false);
    assert.equal(await readFile(file, 'utf8'), before);

    // A change to one asset mapping must produce a real rewrite.
    const updated = fixtureResolvers();
    updated[0] = async () => ({
      name: 'mihomo',
      version: 'v9.9.9',
      archMapping: { 'windows-x86_64': 'mihomo-{}.zip' } as ArchMapping,
    });
    const changed = await generateLatestVersion({
      ...options,
      resolve: updated,
    });
    assert.equal(changed.changed, true);
    assert.equal(
      JSON.parse(await readFile(file, 'utf8')).latest.mihomo,
      'v9.9.9',
    );
  } finally {
    await rm(root, { recursive: true, force: true });
  }
});

test('resolver failures never overwrite the previous manifest', async () => {
  const root = await mkdtemp(path.join(tmpdir(), 'chimera-manifest-failure-'));
  const file = path.join(root, 'manifest/version.json');
  try {
    await mkdir(path.dirname(file), { recursive: true });
    await writeFile(file, '{"keep":"previous"}\n');
    await assert.rejects(
      generateLatestVersion({
        workspaceRoot: root,
        resolve: [
          async () => {
            throw new Error('offline fixture');
          },
        ],
      }),
      /offline fixture/,
    );
    assert.equal(await readFile(file, 'utf8'), '{"keep":"previous"}\n');
  } finally {
    await rm(root, { recursive: true, force: true });
  }
});

test('Node Chimera Client resolvers preserve branded release endpoints and asset names', async () => {
  const previousFetch = globalThis.fetch;
  const requests: string[] = [];
  try {
    globalThis.fetch = (async (url: string | URL | Request) => {
      const endpoint = String(url);
      requests.push(endpoint);
      if (endpoint.endsWith('/releases/latest')) {
        return Response.json({ tag_name: 'v2.4.0' });
      }
      if (endpoint.endsWith('/git/ref/tags/latest')) {
        return Response.json({
          object: { type: 'commit', sha: 'abcdef0123456789' },
        });
      }
      if (endpoint.endsWith('/releases/tags/latest')) {
        return Response.json({
          assets: [
            { name: 'chimera-client-0.26.4-aarch64-apple-darwin.tar.gz' },
          ],
        });
      }
      throw new Error(`Unexpected release request ${endpoint}`);
    }) as typeof fetch;
    const stable = await resolveChimeraClient();
    const alpha = await resolveChimeraClientAlpha();
    assert.equal(stable.name, 'chimera_client');
    assert.equal(stable.version, 'v2.4.0');
    assert.equal(
      stable.archMapping['windows-x86_64'],
      'clash_chimera-x86_64-pc-windows-msvc.exe',
    );
    assert.equal(alpha.name, 'chimera_client_alpha');
    assert.equal(alpha.version, '0.26.4-alpha+sha.abcdef0');
    assert.equal(requests.length, 3);
    assert.ok(
      requests.every((request) => request.includes('/MFSGA/Chimera_Client/')),
    );
  } finally {
    globalThis.fetch = previousFetch;
  }
});
