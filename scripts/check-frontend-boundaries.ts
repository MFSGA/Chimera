import path from 'node:path';
import fs from 'fs-extra';

const workspaceRoot = process.cwd();
const frontendRoots = [
  path.join(workspaceRoot, 'frontend/interface/src'),
  path.join(workspaceRoot, 'frontend/chimera/src'),
];
const generatedBindings = path.normalize(
  path.join(workspaceRoot, 'frontend/interface/src/ipc/bindings.ts'),
);

const SOURCE_EXTENSIONS = new Set(['.ts', '.tsx']);
const violations: string[] = [];

const visit = async (entryPath: string): Promise<void> => {
  const stat = await fs.stat(entryPath);

  if (stat.isDirectory()) {
    const entries = await fs.readdir(entryPath);
    await Promise.all(
      entries.map((entry) => visit(path.join(entryPath, entry))),
    );
    return;
  }

  if (!SOURCE_EXTENSIONS.has(path.extname(entryPath))) {
    return;
  }

  if (path.normalize(entryPath) === generatedBindings) {
    return;
  }

  const source = await fs.readFile(entryPath, 'utf8');
  const relativePath = path.relative(workspaceRoot, entryPath);

  if (
    /from\s+['"]@tauri-apps\/api\/core['"]/.test(source) ||
    /import\s*\(\s*['"]@tauri-apps\/api\/core['"]\s*\)/.test(source)
  ) {
    violations.push(
      `${relativePath}: import Tauri core commands through @chimera/interface instead of @tauri-apps/api/core`,
    );
  }

  if (/\b__?TAURI_INVOKE\s*\(/.test(source)) {
    violations.push(
      `${relativePath}: raw Tauri invoke is reserved for generated IPC bindings`,
    );
  }
};

await Promise.all(frontendRoots.map(visit));

if (violations.length > 0) {
  throw new Error(
    `frontend application interface boundary violations:\n${violations
      .map((violation) => `- ${violation}`)
      .join('\n')}`,
  );
}

console.log(
  'frontend boundaries verified: main and legacy UI share generated application IPC',
);
