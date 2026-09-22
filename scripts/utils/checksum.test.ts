import assert from 'node:assert/strict';
import { createHash } from 'node:crypto';
import { mkdtemp, rm, writeFile } from 'node:fs/promises';
import os from 'node:os';
import path from 'node:path';
import test from 'node:test';
import { parseSha256Digest, verifyFileSha256 } from './checksum.ts';

const digest = '0123456789abcdef'.repeat(4);

test('parses sha256sum output', () => {
  assert.equal(
    parseSha256Digest(
      `${digest}  chimera-service-x86_64-pc-windows-msvc.zip\n`,
    ),
    digest,
  );
});

test('parses PowerShell Format-List output and UTF-16-style nulls', () => {
  const output = [
    'Algorithm : SHA256',
    `Hash      : ${digest.toUpperCase()}`,
    'Path      : C:\\tmp\\chimera-service.zip',
  ].join('\r\n');
  assert.equal(parseSha256Digest(output), digest);
  assert.equal(
    parseSha256Digest([...output].map((char) => `${char}\0`).join('')),
    digest,
  );
});

test('rejects ambiguous or missing checksum content', () => {
  assert.throws(() => parseSha256Digest('no digest here'), /exactly one/);
  assert.throws(
    () => parseSha256Digest(`${digest}\n${'f'.repeat(64)}\n`),
    /exactly one/,
  );
});

test('verifies archive bytes and rejects a mismatch', async () => {
  const dir = await mkdtemp(path.join(os.tmpdir(), 'chimera-checksum-'));
  try {
    const archive = path.join(dir, 'service.zip');
    const checksum = path.join(dir, 'service.zip.sha256');
    const bytes = Buffer.from('service release archive');
    const expected = createHash('sha256').update(bytes).digest('hex');

    await writeFile(archive, bytes);
    await writeFile(checksum, `${expected}  service.zip\n`);
    await verifyFileSha256(archive, checksum);

    await writeFile(checksum, `${'0'.repeat(64)}  service.zip\n`);
    await assert.rejects(
      verifyFileSha256(archive, checksum),
      /SHA-256 mismatch/,
    );
  } finally {
    await rm(dir, { recursive: true, force: true });
  }
});
