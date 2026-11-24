import assert from 'node:assert/strict';
import { randomUUID } from 'node:crypto';
import { readFile } from 'node:fs/promises';
import { tmpdir } from 'node:os';
import { join } from 'node:path';
import { test } from 'node:test';
import { isNotFound } from './errors.ts';

test('isNotFound recognizes a missing path only', async () => {
  const missing = await readFile(join(tmpdir(), randomUUID())).catch((e) => e);
  assert.deepStrictEqual(isNotFound(missing), true);
  assert.deepStrictEqual(isNotFound(new Error('other')), false);
  assert.deepStrictEqual(isNotFound('ENOENT'), false);
});
