// Extract all data-slot attribute values from frontend TSX files.
// Run: pnpm run generate:data-slots
import { glob, mkdir, readFile, writeFile } from 'node:fs/promises';
import { dirname, join } from 'node:path';
import { WORKSPACE_ROOT } from './src/shared/repo-paths.ts';

const GENERATED_PATH = 'frontend/chimera/src/generated/data-slots.gen.ts';

/**
 * Keep Chimera's existing frontend/chimera scan scope during the Node migration.
 * Node 24's built-in glob avoids changing the unrelated pnpm lockfile merely
 * to replace the upstream globby dependency.
 */
export async function collectDataSlots(workspaceRoot = WORKSPACE_ROOT) {
  const slots = new Set<string>();
  for await (const file of glob('frontend/chimera/src/**/*.tsx', {
    cwd: workspaceRoot,
  })) {
    const content = await readFile(join(workspaceRoot, file), 'utf8');
    for (const match of content.matchAll(/data-slot="([^"]+)"/g)) {
      slots.add(match[1]);
    }
  }
  return [...slots].sort();
}

export function renderDataSlots(slots: readonly string[]) {
  // Retain the existing generated banner byte-for-byte until generated files
  // are reviewed and updated separately.
  return (
    '// AUTO-GENERATED — do not edit manually. Run: pnpm generate:data-slots\n' +
    `export const DATA_SLOTS = ${JSON.stringify(slots, null, 2)} as const\n\n` +
    'export type DataSlot = (typeof DATA_SLOTS)[number]\n'
  );
}

export async function generateDataSlots(workspaceRoot = WORKSPACE_ROOT) {
  const slots = await collectDataSlots(workspaceRoot);
  const outputPath = join(workspaceRoot, GENERATED_PATH);
  await mkdir(dirname(outputPath), { recursive: true });
  await writeFile(outputPath, renderDataSlots(slots));
  console.log(
    `[extract-data-slots] ${slots.length} slots written to ${outputPath}`,
  );
  return slots;
}

if (import.meta.main) {
  await generateDataSlots();
}
