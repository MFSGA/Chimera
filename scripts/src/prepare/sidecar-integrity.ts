import { createHash, randomUUID } from 'node:crypto';
import { createReadStream } from 'node:fs';
import { readFile, rename, rm, writeFile } from 'node:fs/promises';
import path from 'node:path';

export interface VerifyAndInstallSidecarOptions {
  archivePath: string;
  checksumPath?: string;
  targetPath: string;
  versionStampPath?: string;
  version?: string;
  stage: (stagedPath: string) => Promise<void> | void;
}

export function parseSha256Checksum(checksumText: string): string {
  const powershellHash = checksumText.match(
    /^\s*Hash\s*:\s*([a-f\d]{64})\s*$/im,
  )?.[1];
  const sha256sumHash = checksumText.match(
    /^\s*([a-f\d]{64})(?:\s+\*?[^\r\n]*)?\s*$/im,
  )?.[1];
  const hash = powershellHash ?? sha256sumHash;

  if (!hash) {
    throw new Error('checksum file does not contain a SHA-256 digest');
  }

  return hash.toLowerCase();
}

export async function sha256File(filePath: string): Promise<string> {
  const hash = createHash('sha256');
  for await (const chunk of createReadStream(filePath)) {
    hash.update(chunk);
  }

  return hash.digest('hex');
}

export async function verifyAndInstallSidecar({
  archivePath,
  checksumPath,
  targetPath,
  versionStampPath,
  version,
  stage,
}: VerifyAndInstallSidecarOptions): Promise<void> {
  if (checksumPath) {
    const checksumText = await readFile(checksumPath, 'utf8');
    const expectedHash = parseSha256Checksum(checksumText);
    const actualHash = await sha256File(archivePath);

    if (actualHash !== expectedHash) {
      throw new Error(
        `SHA-256 mismatch for ${path.basename(archivePath)}: expected ${expectedHash}, got ${actualHash}`,
      );
    }
  }

  const stagedPath = path.join(
    path.dirname(targetPath),
    `.${path.basename(targetPath)}.${randomUUID()}.tmp`,
  );

  try {
    await stage(stagedPath);
    // Keep the candidate beside the target so rename stays on one filesystem.
    // The adjacent staged candidate keeps rename on the same filesystem. A failed rename leaves the
    // previous target untouched.
    await rename(stagedPath, targetPath);

    if (versionStampPath && version) {
      const stagedStampPath = `${versionStampPath}.${randomUUID()}.tmp`;
      try {
        await writeFile(stagedStampPath, version, { flag: 'wx' });
        await rename(stagedStampPath, versionStampPath);
      } finally {
        try {
          await rm(stagedStampPath, { force: true });
        } catch {
          // ignore cleanup when rename already moved the file
        }
      }
    }
  } finally {
    try {
      await rm(stagedPath, { force: true });
    } catch {
      // ignore cleanup when rename already moved the file
    }
  }
}
