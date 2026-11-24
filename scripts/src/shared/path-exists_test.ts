import assert from 'node:assert/strict';
import { randomUUID } from 'node:crypto';
import { tmpdir } from 'node:os';
import { join } from 'node:path';
import { test } from 'node:test';
import { fileURLToPath } from 'node:url';
import { pathExists } from './path-exists.ts';

test('pathExists reports existing and missing paths', async () => {
  assert.deepStrictEqual(
    await pathExists(fileURLToPath(import.meta.url)),
    true,
  );
  assert.deepStrictEqual(await pathExists(join(tmpdir(), randomUUID())), false);
});
