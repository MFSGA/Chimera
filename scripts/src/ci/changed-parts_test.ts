import assert from 'node:assert/strict';
import { changedParts, partsOf, type Part } from './changed-parts.ts';

const sorted = (parts: Iterable<Part>) => [...parts].sort();

Deno.test('files map to the parts that check them', () => {
  const cases: [string, Part[]][] = [
    ['README.md', []],
    ['docs/testing/README.md', []],
    ['frontend/chimera/src/main.tsx', ['frontend']],
    ['tauri-e2e/specs/smoke.e2e.ts', ['frontend']],
    ['frontend/chimera/README.md', ['frontend']],
    ['package.json', ['backend', 'frontend', 'scripts']],
    ['pnpm-lock.yaml', ['backend', 'frontend', 'scripts']],
    ['frontend/interface/src/ipc/bindings.ts', ['frontend']],
    ['backend/tauri/src/lib.rs', ['backend']],
    ['backend/chimera-runtime', ['backend']],
    ['backend/Cargo.lock', ['backend']],
    ['rust-toolchain.toml', ['backend']],
    ['scripts/src/ci/changed-parts.ts', ['scripts']],
    ['scripts/deno.lock', ['scripts']],
    ['.github/workflows/target-release-build.yaml', ['scripts']],
  ];

  for (const [file, parts] of cases) {
    assert.deepEqual(sorted(partsOf(file)), sorted(parts), file);
  }
});

Deno.test('unclassified files check every part', () => {
  for (const file of [
    '.github/workflows/ci.yaml',
    '.gitattributes',
    'new-tool/src/main.rs',
  ]) {
    assert.deepEqual(sorted(partsOf(file)), ['backend', 'frontend', 'scripts']);
  }
});

Deno.test('a change checks the union of its files parts', () => {
  assert.deepEqual(
    sorted(
      changedParts([
        'package.json',
        'frontend/chimera/package.json',
        'backend/Cargo.lock',
      ]),
    ),
    ['backend', 'frontend', 'scripts'],
  );
  assert.deepEqual(sorted(changedParts(['README.md', 'docs/a.md'])), []);
  assert.deepEqual(sorted(changedParts([])), []);
});
