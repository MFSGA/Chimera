import { execFileSync } from 'node:child_process';
import path from 'node:path';
import fs from 'fs-extra';
import { getChimeraServiceVersion } from './utils/chimera-service-resource';
import { SIDECAR_HOST } from './utils/consts';
import { cwd, TAURI_APP_DIR } from './utils/env';

const serviceExecutableName =
  process.platform === 'win32' ? 'chimera-service.exe' : 'chimera-service';

const targetDir = path.join(cwd, 'backend/target/e2e');
const builtService = path.join(targetDir, 'debug', serviceExecutableName);

if (!SIDECAR_HOST) {
  throw new Error('failed to resolve Rust host triple for E2E Service sidecar');
}

execFileSync(
  'cargo',
  [
    '+nightly',
    'build',
    '--manifest-path',
    'backend/chimera-runtime/Cargo.toml',
    '-p',
    'chimera-service',
    '--target-dir',
    targetDir,
  ],
  {
    cwd,
    stdio: 'inherit',
    windowsHide: true,
  },
);

if (!(await fs.pathExists(builtService))) {
  throw new Error('current E2E Service build did not produce ' + builtService);
}

const expectedVersion = (await getChimeraServiceVersion()).replace(/^v/, '');
const versionOutput = execFileSync(builtService, ['--version'], {
  cwd,
  encoding: 'utf8',
  windowsHide: true,
}).trim();
if (!versionOutput.includes(expectedVersion)) {
  throw new Error(
    `E2E Service version mismatch: expected ${expectedVersion}, got ${versionOutput}`,
  );
}

const sidecarDir = path.join(TAURI_APP_DIR, 'sidecar');
const sidecarTarget = path.join(
  sidecarDir,
  `chimera-service-${SIDECAR_HOST}${process.platform === 'win32' ? '.exe' : ''}`,
);
await fs.mkdirp(sidecarDir);
await fs.copy(builtService, sidecarTarget, { overwrite: true });

console.log(
  `staged current Chimera Service ${expectedVersion} for E2E: ${sidecarTarget}`,
);
