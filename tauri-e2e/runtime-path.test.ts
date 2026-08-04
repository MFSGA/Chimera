import assert from 'node:assert/strict';
import fs from 'node:fs';
import os from 'node:os';
import path from 'node:path';
import test from 'node:test';
import { resolveRuntimeDirectory } from './runtime-path.js';

test('consecutive E2E runs receive different runtime directories', () => {
  const root = path.join(os.tmpdir(), 'chimera-e2e-runtime-test');
  const first = resolveRuntimeDirectory(root, undefined, 'run-one');
  const second = resolveRuntimeDirectory(root, undefined, 'run-two');

  assert.notEqual(first, second);
  assert.equal(first, path.join(root, 'run-one'));
  assert.equal(second, path.join(root, 'run-two'));
});

test('a later run cannot see sentinel data from an earlier run', () => {
  const root = fs.mkdtempSync(path.join(os.tmpdir(), 'chimera-e2e-'));
  const first = resolveRuntimeDirectory(root, undefined, 'run-one');
  const second = resolveRuntimeDirectory(root, undefined, 'run-two');

  fs.mkdirSync(first, { recursive: true });
  fs.writeFileSync(path.join(first, 'sentinel.txt'), 'previous run');
  fs.mkdirSync(second, { recursive: true });

  assert.equal(fs.existsSync(path.join(second, 'sentinel.txt')), false);
});

test('an explicit runtime override is preserved', () => {
  const root = path.join(os.tmpdir(), 'unused-root');
  const override = path.join(os.tmpdir(), 'chimera-explicit-runtime');

  assert.equal(
    resolveRuntimeDirectory(root, override, 'ignored'),
    path.resolve(override),
  );
});
