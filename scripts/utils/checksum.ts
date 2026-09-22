import { createHash } from 'node:crypto';
import { readFile } from 'node:fs/promises';

const SHA256_PATTERN = /[a-fA-F0-9]{64}/g;

export function parseSha256Digest(raw: string): string {
  const normalized = raw.replaceAll('\0', '');
  const matches = normalized.match(SHA256_PATTERN) ?? [];
  const unique = [...new Set(matches.map((value) => value.toLowerCase()))];

  if (unique.length !== 1) {
    throw new Error(
      `expected exactly one SHA-256 digest, found ${unique.length}`,
    );
  }

  return unique[0];
}

export async function verifyFileSha256(
  filePath: string,
  checksumPath: string,
): Promise<void> {
  const [file, checksum] = await Promise.all([
    readFile(filePath),
    readFile(checksumPath, 'utf8'),
  ]);
  const expected = parseSha256Digest(checksum);
  const actual = createHash('sha256').update(file).digest('hex');

  if (actual !== expected) {
    throw new Error(
      `SHA-256 mismatch for ${filePath}: expected ${expected}, got ${actual}`,
    );
  }
}
