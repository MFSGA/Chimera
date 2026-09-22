import assert from 'node:assert/strict';
import test from 'node:test';
import { canReuseExistingSidecar } from './sidecar-cache.ts';

test('unversioned sidecars may reuse an existing canonical target', () => {
  assert.equal(canReuseExistingSidecar({ targetExists: true }), true);
});

test('version-pinned sidecars never trust existence alone', () => {
  assert.equal(
    canReuseExistingSidecar({
      targetExists: true,
      version: 'v1.10.0',
    }),
    false,
  );
});

test('force disables cache reuse', () => {
  assert.equal(
    canReuseExistingSidecar({
      force: true,
      targetExists: true,
    }),
    false,
  );
});
