import assert from 'node:assert/strict';
import fs from 'node:fs';
import path from 'node:path';
import test from 'node:test';
import { fileURLToPath } from 'node:url';
import { baseE2eSuites, e2eSuites } from './spec-suites.ts';

const configDirectory = path.dirname(fileURLToPath(import.meta.url));
const specsDirectory = path.join(configDirectory, 'specs');

function collectSpecs(directory: string): string[] {
  return fs
    .readdirSync(directory, { withFileTypes: true })
    .flatMap((entry) => {
      const fullPath = path.join(directory, entry.name);
      if (entry.isDirectory()) return collectSpecs(fullPath);
      if (!entry.isFile() || !entry.name.endsWith('.e2e.ts')) return [];
      return [
        `./${path
          .relative(configDirectory, fullPath)
          .split(path.sep)
          .join('/')}`,
      ];
    })
    .sort();
}

test('base E2E suites partition every spec exactly once', () => {
  const discovered = collectSpecs(specsDirectory);
  const grouped = Object.values(baseE2eSuites).flat().sort();

  assert.equal(
    new Set(grouped).size,
    grouped.length,
    'suite contains duplicates',
  );
  assert.deepEqual(grouped, discovered);
});

test('critical and hermetic suites have intentional coverage boundaries', () => {
  assert.deepEqual(
    [...e2eSuites.critical].sort(),
    [
      ...baseE2eSuites.smoke,
      ...baseE2eSuites.runtime,
      ...baseE2eSuites.profiles,
    ].sort(),
  );
  assert.equal(
    e2eSuites.hermetic.includes(baseE2eSuites.network[0]),
    false,
    'LAN test must stay out of hermetic CI because it requires an external WSL/LAN client',
  );
  assert.deepEqual(
    [...e2eSuites.all].sort(),
    Object.values(baseE2eSuites).flat().sort(),
  );
});
