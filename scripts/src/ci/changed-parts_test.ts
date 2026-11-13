import assert from 'node:assert/strict';
import { execFile } from 'node:child_process';
import { mkdtemp, readFile, rm } from 'node:fs/promises';
import { tmpdir } from 'node:os';
import * as path from 'node:path';
import { test } from 'node:test';
import { promisify } from 'node:util';
import { WORKSPACE_ROOT } from '../shared/repo-paths.ts';
import { changedParts, partsOf, type Part } from './changed-parts.ts';

const sorted = (parts: Iterable<Part>) => [...parts].sort();

test('files map to the parts that check them', () => {
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
    ['scripts/tsconfig.prepare.json', ['scripts']],
    ['manifest/version.json', ['backend', 'frontend', 'scripts']],
    ['frontend/chimera/src/nested/settings.tsx', ['frontend']],
    ['.github/workflows/target-release-build.yaml', ['scripts']],
  ];

  for (const [file, parts] of cases) {
    assert.deepEqual(sorted(partsOf(file)), sorted(parts), file);
  }
});

test('unclassified files check every part', () => {
  for (const file of [
    '.github/workflows/ci.yaml',
    '.gitattributes',
    'new-tool/src/main.rs',
  ]) {
    assert.deepEqual(sorted(partsOf(file)), ['backend', 'frontend', 'scripts']);
  }
});

test('a change checks the union of its files parts', () => {
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

const execFileAsync = promisify(execFile);

test('Node CLI reports changes and appends GITHUB_OUTPUT with original semantics', async () => {
  const temp = await mkdtemp(path.join(tmpdir(), 'chimera-changed-parts-'));
  const output = path.join(temp, 'github-output.txt');
  const cliPath = path.join(WORKSPACE_ROOT, 'scripts/src/ci/changed-parts.ts');
  const env = { ...process.env, GITHUB_OUTPUT: output };
  try {
    const full = await execFileAsync(process.execPath, [cliPath], {
      cwd: temp,
      env,
    });
    assert.equal(
      full.stdout.trim(),
      'frontend=true\nbackend=true\nscripts=true',
    );
    assert.equal(
      await readFile(output, 'utf8'),
      'frontend=true\nbackend=true\nscripts=true\n',
    );

    const unchanged = await execFileAsync(process.execPath, [cliPath, 'HEAD'], {
      cwd: temp,
      env,
    });
    assert.equal(
      unchanged.stdout.trim(),
      'frontend=false\nbackend=false\nscripts=false',
    );
    assert.equal(
      await readFile(output, 'utf8'),
      'frontend=true\nbackend=true\nscripts=true\n' +
        'frontend=false\nbackend=false\nscripts=false\n',
    );

    await assert.rejects(
      execFileAsync(process.execPath, [cliPath, 'chimera-nonexistent-ref'], {
        cwd: temp,
        env,
      }),
    );
    // An invalid Git base must not append a misleading successful classification.
    assert.equal(
      await readFile(output, 'utf8'),
      'frontend=true\nbackend=true\nscripts=true\n' +
        'frontend=false\nbackend=false\nscripts=false\n',
    );
  } finally {
    await rm(temp, { recursive: true, force: true });
  }
});
