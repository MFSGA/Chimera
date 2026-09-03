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
const legacyUiRoots = [
  path.normalize(
    path.join(workspaceRoot, 'frontend/chimera/src/pages/(legacy)'),
  ),
  path.normalize(
    path.join(workspaceRoot, 'frontend/chimera/src/components/dashboard'),
  ),
  path.normalize(
    path.join(workspaceRoot, 'frontend/chimera/src/components/setting'),
  ),
];
const sharedClashBaseUiFiles = new Set(
  ['frontend/chimera/src/components/setting/setting-clash-base.tsx'].map(
    (entry) => path.normalize(path.join(workspaceRoot, entry)),
  ),
);
const sharedThemeUiFiles = new Set(
  ['frontend/chimera/src/components/setting/setting-chimera-ui.tsx'].map(
    (entry) => path.normalize(path.join(workspaceRoot, entry)),
  ),
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

  const normalizedEntryPath = path.normalize(entryPath);
  const isLegacyUi = legacyUiRoots.some(
    (legacyRoot) =>
      normalizedEntryPath === legacyRoot ||
      normalizedEntryPath.startsWith(`${legacyRoot}${path.sep}`),
  );

  if (isLegacyUi && /\bcommands\s*\./.test(source)) {
    violations.push(
      `${relativePath}: legacy UI must adapt through shared features/hooks instead of calling generated commands directly`,
    );
  }

  if (
    sharedClashBaseUiFiles.has(normalizedEntryPath) &&
    /\buseClashConfig\b/.test(source)
  ) {
    violations.push(
      `${relativePath}: Clash base UI must use the shared clash-settings feature instead of useClashConfig directly`,
    );
  }

  if (
    sharedThemeUiFiles.has(normalizedEntryPath) &&
    /\buseSetting\b/.test(source)
  ) {
    violations.push(
      `${relativePath}: legacy theme settings must use the shared theme provider instead of useSetting directly`,
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
