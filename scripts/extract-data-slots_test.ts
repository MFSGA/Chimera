import assert from 'node:assert/strict';
import { execFile } from 'node:child_process';
import {
  copyFile,
  mkdir,
  mkdtemp,
  readFile,
  rm,
  writeFile,
} from 'node:fs/promises';
import { tmpdir } from 'node:os';
import * as path from 'node:path';
import { test } from 'node:test';
import { pathToFileURL } from 'node:url';
import { promisify } from 'node:util';
import {
  collectDataSlots,
  generateDataSlots,
  renderDataSlots,
} from './extract-data-slots.ts';
import { WORKSPACE_ROOT } from './src/shared/repo-paths.ts';

const execFileAsync = promisify(execFile);

test('data-slot extraction keeps the Chimera TSX scope and sorts unique names', async () => {
  const root = await mkdtemp(path.join(tmpdir(), 'chimera-data-slots-'));
  try {
    const main = path.join(root, 'frontend/chimera/src');
    const shared = path.join(root, 'frontend/ui/src');
    await mkdir(path.join(main, 'nested'), { recursive: true });
    await mkdir(shared, { recursive: true });
    await writeFile(
      path.join(main, 'a.tsx'),
      '<div data-slot="zeta" data-slot="alpha" data-slot=\'single\' />',
    );
    await writeFile(
      path.join(main, 'nested/b.tsx'),
      '<span data-slot="alpha" data-slot="beta" />',
    );
    await writeFile(path.join(main, 'not-tsx.ts'), 'data-slot="skip-ts"');
    await writeFile(path.join(shared, 'shared.tsx'), 'data-slot="skip-ui"');

    const slots = await collectDataSlots(root);
    assert.deepEqual(slots, ['alpha', 'beta', 'zeta']);

    const written = await generateDataSlots(root);
    assert.deepEqual(written, slots);
    assert.equal(
      await readFile(path.join(main, 'generated/data-slots.gen.ts'), 'utf8'),
      renderDataSlots(slots),
    );
    assert.equal(
      renderDataSlots(['alpha']),
      '// AUTO-GENERATED — do not edit manually. Run: pnpm generate:data-slots\n' +
        'export const DATA_SLOTS = [\n  "alpha"\n] as const\n\n' +
        'export type DataSlot = (typeof DATA_SLOTS)[number]\n',
    );
  } finally {
    await rm(root, { recursive: true, force: true });
  }
});

test('importing the Node generator does not write generated files', async () => {
  const root = await mkdtemp(path.join(tmpdir(), 'chimera-data-slots-import-'));
  const scriptPath = path.join(root, 'scripts/extract-data-slots.ts');
  const repoPathsPath = path.join(root, 'scripts/src/shared/repo-paths.ts');
  const generatedPath = path.join(
    root,
    'frontend/chimera/src/generated/data-slots.gen.ts',
  );
  const marker = 'generated output must remain untouched';

  try {
    await writeFile(path.join(root, 'package.json'), '{"type":"module"}');
    await mkdir(path.dirname(repoPathsPath), { recursive: true });
    await mkdir(path.dirname(generatedPath), { recursive: true });
    await copyFile(
      path.join(WORKSPACE_ROOT, 'scripts/extract-data-slots.ts'),
      scriptPath,
    );
    await copyFile(
      path.join(WORKSPACE_ROOT, 'scripts/src/shared/repo-paths.ts'),
      repoPathsPath,
    );
    await writeFile(generatedPath, marker);

    await execFileAsync(
      process.execPath,
      [
        '--import',
        'tsx',
        '--eval',
        `await import(${JSON.stringify(pathToFileURL(scriptPath).href)})`,
      ],
      { cwd: WORKSPACE_ROOT },
    );

    assert.equal(await readFile(generatedPath, 'utf8'), marker);
  } finally {
    await rm(root, { recursive: true, force: true });
  }

  assert.equal(typeof collectDataSlots, 'function');
  assert.equal(typeof generateDataSlots, 'function');
});
