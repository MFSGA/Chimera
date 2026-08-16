import assert from 'node:assert/strict';
import fs from 'node:fs';
import os from 'node:os';
import path from 'node:path';
import test from 'node:test';
import {
  cleanupRuntimeDirectory,
  pruneRuntimeDirectories,
  resolveRuntimeDirectory,
} from './runtime-path.ts';

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
  fs.rmSync(root, { recursive: true, force: true });
});

test('an explicit runtime override is preserved', () => {
  const root = path.join(os.tmpdir(), 'unused-root');
  const override = path.join(os.tmpdir(), 'chimera-explicit-runtime');

  assert.equal(
    resolveRuntimeDirectory(root, override, 'ignored'),
    path.resolve(override),
  );
});

test('runtime cleanup removes only a direct generated child', () => {
  const root = fs.mkdtempSync(path.join(os.tmpdir(), 'chimera-e2e-cleanup-'));
  const runtime = path.join(root, 'run-one');
  const nested = path.join(runtime, 'nested');
  const outside = fs.mkdtempSync(
    path.join(os.tmpdir(), 'chimera-e2e-outside-'),
  );

  fs.mkdirSync(nested, { recursive: true });
  fs.writeFileSync(path.join(nested, 'sentinel.txt'), 'data');

  assert.equal(cleanupRuntimeDirectory(root, root), false);
  assert.equal(cleanupRuntimeDirectory(root, nested), false);
  assert.equal(cleanupRuntimeDirectory(root, outside), false);
  assert.equal(cleanupRuntimeDirectory(root, runtime), true);
  assert.equal(fs.existsSync(runtime), false);
  assert.equal(fs.existsSync(outside), true);

  fs.rmSync(root, { recursive: true, force: true });
  fs.rmSync(outside, { recursive: true, force: true });
});

test('runtime pruning removes stale runs but preserves fresh and active runs', () => {
  const root = fs.mkdtempSync(path.join(os.tmpdir(), 'chimera-e2e-prune-'));
  const stale = path.join(root, 'stale');
  const fresh = path.join(root, 'fresh');
  const active = path.join(root, 'active');
  const marker = path.join(root, 'marker.txt');
  const now = Date.now();

  fs.mkdirSync(stale);
  fs.mkdirSync(fresh);
  fs.mkdirSync(active);
  fs.writeFileSync(marker, 'not a runtime directory');
  const staleTime = new Date(now - 2 * 60 * 60 * 1000);
  fs.utimesSync(stale, staleTime, staleTime);

  const removed = pruneRuntimeDirectories(root, {
    olderThanMs: 60 * 60 * 1000,
    now,
    exclude: [active],
  });

  assert.deepEqual(removed, [stale]);
  assert.equal(fs.existsSync(stale), false);
  assert.equal(fs.existsSync(fresh), true);
  assert.equal(fs.existsSync(active), true);
  assert.equal(fs.existsSync(marker), true);

  fs.rmSync(root, { recursive: true, force: true });
});
