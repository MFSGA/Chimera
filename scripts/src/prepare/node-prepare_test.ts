import assert from 'node:assert/strict';
import { createHash } from 'node:crypto';
import { mkdir, mkdtemp, readFile, rm, writeFile } from 'node:fs/promises';
import { tmpdir } from 'node:os';
import * as path from 'node:path';
import { test } from 'node:test';
import { gzipSync } from 'node:zlib';
import AdmZip from 'adm-zip';
import { WORKSPACE_ROOT } from '../shared/repo-paths.ts';
import { createBinaryResolvers } from './binaries.ts';
import { downloadFile, extractZip, gunzipFile } from './download.ts';
import { resolveResource, resolveSidecar } from './resources.ts';
import type { VersionManifest } from './types.ts';

async function withFixture(
  run: (root: string) => Promise<void>,
): Promise<void> {
  const root = await mkdtemp(path.join(tmpdir(), 'chimera-prepare-node-'));
  const previousFetch = globalThis.fetch;
  try {
    await run(root);
  } finally {
    globalThis.fetch = previousFetch;
    await rm(root, { recursive: true, force: true });
  }
}

function context(root: string) {
  return {
    resourcesDir: path.join(root, 'resources'),
    sidecarDir: path.join(root, 'sidecar'),
    tempRoot: path.join(root, 'tmp'),
    platform: 'win32',
    arch: 'x64',
  };
}

test('Node entry is importable and has a source-based workspace root', async () => {
  const { main } = await import('./check.ts');
  assert.equal(typeof main, 'function');
  assert.equal(
    path.resolve(WORKSPACE_ROOT).toLowerCase(),
    path.resolve(import.meta.dirname, '../../..').toLowerCase(),
  );
});

test('Node Chimera Client Alpha refresh uses direct GitHub metadata', async () => {
  await withFixture(async (root) => {
    const requests: string[] = [];
    globalThis.fetch = (async (url: string | URL | Request) => {
      const requested = String(url);
      requests.push(requested);
      if (requested.endsWith('/git/ref/tags/latest')) {
        return Response.json({
          object: { type: 'commit', sha: 'abcdef0123456789' },
        });
      }
      if (requested.endsWith('/releases/tags/latest')) {
        return Response.json({
          assets: [
            { name: 'chimera-client-0.26.4-aarch64-apple-darwin.tar.gz' },
          ],
        });
      }
      throw new Error(`Unexpected GitHub request ${requested}`);
    }) as typeof fetch;
    const manifest = {
      latest: { chimera_client_alpha: '0.26.3-alpha+sha.1234567' },
      arch_template: {
        chimera_client_alpha: {
          'windows-x86_64': 'clash_chimera-x86_64-pc-windows-msvc.exe',
        },
      },
    } as unknown as VersionManifest;
    const binary = createBinaryResolvers({
      versionManifest: manifest,
      sidecarHost: 'x86_64-pc-windows-msvc',
      platform: 'win32',
      arch: 'x64',
      workspaceRoot: root,
    });
    const refreshed = await binary.chimeraClientAlpha().refresh!();
    assert.equal(refreshed.version, '0.26.4-alpha+sha.abcdef0');
    assert.match(
      refreshed.downloadURL,
      /Chimera_Client\/releases\/download\/latest\//,
    );
    assert.equal(requests.length, 2);
  });
});

test('Node stream writes complete chunks and reports progress', async () => {
  await withFixture(async (root) => {
    globalThis.fetch = (async () =>
      new Response(
        new ReadableStream<Uint8Array>({
          start(controller) {
            controller.enqueue(new Uint8Array([1, 2]));
            controller.enqueue(new Uint8Array([3, 4, 5]));
            controller.close();
          },
        }),
        { headers: { 'content-length': '5' } },
      )) as typeof fetch;
    const actual: number[] = [];
    const output = path.join(root, 'download.bin');
    const result = await downloadFile(
      'https://example.test/download.bin',
      output,
      (progress) => actual.push(progress.downloaded),
    );
    assert.deepEqual(await readFile(output), Buffer.from([1, 2, 3, 4, 5]));
    assert.equal(result.size, 5);
    assert.deepEqual(actual, [2, 5]);
  });
});

test('Node ZIP extraction and gzip decompression retain executable bytes', async () => {
  await withFixture(async (root) => {
    const archive = new AdmZip();
    archive.addFile('release/chimera-service.exe', Buffer.from('sidecar'));
    const zipPath = path.join(root, 'sidecar.zip');
    archive.writeZip(zipPath);
    const binaryPath = await extractZip(zipPath, root, 'chimera-service');
    assert.equal(await readFile(binaryPath, 'utf8'), 'sidecar');

    const gzipPath = path.join(root, 'sidecar.gz');
    const uncompressedPath = path.join(root, 'uncompressed');
    await writeFile(gzipPath, gzipSync('compressed-payload'));
    await gunzipFile(gzipPath, uncompressedPath);
    assert.equal(
      await readFile(uncompressedPath, 'utf8'),
      'compressed-payload',
    );
  });
});

test('Node resource download failure leaves a cached file unchanged', async () => {
  await withFixture(async (root) => {
    const configuration = context(root);
    await mkdir(configuration.resourcesDir, { recursive: true });
    const target = path.join(configuration.resourcesDir, 'Country.mmdb');
    await writeFile(target, 'previous-version');
    globalThis.fetch = (async () =>
      new Response(
        new ReadableStream<Uint8Array>({
          start(controller) {
            controller.enqueue(new Uint8Array([1, 2]));
            controller.error(new Error('connection reset'));
          },
        }),
      )) as typeof fetch;
    await assert.rejects(
      resolveResource(
        configuration,
        { file: 'Country.mmdb', downloadURL: 'https://example.test/mmdb' },
        { force: true },
      ),
      /connection reset/,
    );
    assert.equal(await readFile(target, 'utf8'), 'previous-version');
  });
});

test('Node sidecar verifies SHA-256 before replacing executable and stamp', async () => {
  await withFixture(async (root) => {
    const configuration = context(root);
    await mkdir(configuration.sidecarDir, { recursive: true });
    const target = path.join(configuration.sidecarDir, 'chimera-service.exe');
    const stamp = `${target}.version`;
    await writeFile(target, 'old executable');
    await writeFile(stamp, 'v0');
    const bytes = Buffer.from('signed replacement');
    const digest = createHash('sha256').update(bytes).digest('hex');

    let validChecksum = false;
    globalThis.fetch = (async (url: string | URL | Request) => {
      const href = String(url);
      return new Response(
        href.endsWith('.sha256')
          ? `${validChecksum ? digest : '0'.repeat(64)}  binary.bin`
          : bytes,
      );
    }) as typeof fetch;

    const info = {
      name: 'chimera-service',
      version: 'v1',
      targetFile: 'chimera-service.exe',
      exeFile: 'binary.bin',
      tmpFile: 'binary.bin',
      downloadURL: 'https://example.test/binary.bin',
      checksumURL: 'https://example.test/binary.bin.sha256',
    };
    await assert.rejects(
      resolveSidecar(configuration, info, { force: true }),
      /SHA-256 mismatch/,
    );
    assert.equal(await readFile(target, 'utf8'), 'old executable');
    assert.equal(await readFile(stamp, 'utf8'), 'v0');

    validChecksum = true;
    const result = await resolveSidecar(configuration, info, { force: true });
    assert.equal(result.cached, false);
    assert.deepEqual(await readFile(target), bytes);
    assert.equal(await readFile(stamp, 'utf8'), 'v1');

    globalThis.fetch = (async () => {
      throw new Error('cache hit must not download');
    }) as typeof fetch;
    assert.equal((await resolveSidecar(configuration, info)).cached, true);
  });
});
