import { strict as assert } from 'node:assert';
import { createHash } from 'node:crypto';
import {
  parseSha256Checksum,
  verifyAndInstallSidecar,
} from './sidecar-integrity.ts';

function digest(content: string): string {
  return createHash('sha256').update(content).digest('hex');
}

Deno.test('parses GNU and PowerShell SHA-256 checksum assets', () => {
  const hash = digest('service archive');

  assert.equal(parseSha256Checksum(`${hash}  chimera-service.tar.gz\n`), hash);
  assert.equal(
    parseSha256Checksum(
      `Algorithm : SHA256\r\nHash : ${hash}\r\nPath : archive.zip\r\n`,
    ),
    hash,
  );
});

Deno.test('rejects malformed checksum assets', () => {
  assert.throws(() => parseSha256Checksum('not a SHA-256 checksum'));
});

Deno.test(
  'checksum mismatch preserves the existing sidecar and version stamp',
  async () => {
    const root = await Deno.makeTempDir({
      prefix: 'chimera-sidecar-integrity-',
    });
    try {
      const archivePath = `${root}/service.tar.gz`;
      const checksumPath = `${archivePath}.sha256`;
      const targetPath = `${root}/chimera-service`;
      const versionStampPath = `${targetPath}.version`;
      await Deno.writeTextFile(archivePath, 'downloaded archive');
      await Deno.writeTextFile(
        checksumPath,
        `${digest('different archive')}  service.tar.gz\n`,
      );
      await Deno.writeTextFile(targetPath, 'known-good sidecar');
      await Deno.writeTextFile(versionStampPath, 'v1');

      let stageCalled = false;
      await assert.rejects(
        verifyAndInstallSidecar({
          archivePath,
          checksumPath,
          targetPath,
          versionStampPath,
          version: 'v2',
          stage: async (stagedPath) => {
            stageCalled = true;
            await Deno.writeTextFile(stagedPath, 'new sidecar');
          },
        }),
        /SHA-256 mismatch/,
      );

      assert.equal(stageCalled, false);
      assert.equal(await Deno.readTextFile(targetPath), 'known-good sidecar');
      assert.equal(await Deno.readTextFile(versionStampPath), 'v1');
    } finally {
      await Deno.remove(root, { recursive: true });
    }
  },
);

Deno.test(
  'verified sidecar is promoted before its version stamp is updated',
  async () => {
    const root = await Deno.makeTempDir({
      prefix: 'chimera-sidecar-integrity-',
    });
    try {
      const archivePath = `${root}/service.zip`;
      const checksumPath = `${archivePath}.sha256`;
      const targetPath = `${root}/chimera-service.exe`;
      const versionStampPath = `${targetPath}.version`;
      await Deno.writeTextFile(archivePath, 'verified archive');
      await Deno.writeTextFile(
        checksumPath,
        `${digest('verified archive')}  service.zip\n`,
      );
      await Deno.writeTextFile(targetPath, 'old sidecar');
      await Deno.writeTextFile(versionStampPath, 'v1');

      await verifyAndInstallSidecar({
        archivePath,
        checksumPath,
        targetPath,
        versionStampPath,
        version: 'v2',
        stage: (stagedPath) => Deno.writeTextFile(stagedPath, 'new sidecar'),
      });

      assert.equal(await Deno.readTextFile(targetPath), 'new sidecar');
      assert.equal(await Deno.readTextFile(versionStampPath), 'v2');
    } finally {
      await Deno.remove(root, { recursive: true });
    }
  },
);

Deno.test('staging failure preserves the existing sidecar', async () => {
  const root = await Deno.makeTempDir({ prefix: 'chimera-sidecar-integrity-' });
  try {
    const archivePath = `${root}/service.tar.gz`;
    const targetPath = `${root}/chimera-service`;
    await Deno.writeTextFile(archivePath, 'archive');
    await Deno.writeTextFile(targetPath, 'known-good sidecar');

    await assert.rejects(
      verifyAndInstallSidecar({
        archivePath,
        targetPath,
        stage: () => {
          throw new Error('extraction failed');
        },
      }),
      /extraction failed/,
    );

    assert.equal(await Deno.readTextFile(targetPath), 'known-good sidecar');
  } finally {
    await Deno.remove(root, { recursive: true });
  }
});
