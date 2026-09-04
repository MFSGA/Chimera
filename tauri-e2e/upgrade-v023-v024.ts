import { spawnSync } from 'node:child_process';
import fs from 'node:fs';
import os from 'node:os';
import path from 'node:path';
import { fileURLToPath } from 'node:url';

const root = path.dirname(fileURLToPath(import.meta.url));
const wdioBin = path.join(
  root,
  'node_modules',
  '@wdio',
  'cli',
  'bin',
  'wdio.js',
);
const config = path.join(root, 'wdio.conf.ts');
const spec = path.join(root, 'specs', 'upgrade-v0.22.3-to-v0.22.4.e2e.ts');
const fixture = path.join(root, 'fixtures', 'upgrade-v0.22.3');
const runtime = fs.mkdtempSync(
  path.join(os.tmpdir(), 'chimera-upgrade-v023-v024-'),
);

function runPhase(phase: 'seed' | 'restart', seedFixture: boolean): void {
  const env = {
    ...process.env,
    CHIMERA_E2E_RUNTIME_DIR: runtime,
    CHIMERA_E2E_UPGRADE_PHASE: phase,
  };

  if (seedFixture) {
    env.CHIMERA_E2E_CONFIG_FIXTURE = fixture;
  } else {
    delete env.CHIMERA_E2E_CONFIG_FIXTURE;
  }

  console.log(`\n=== v0.22.3 -> v0.22.4 upgrade phase: ${phase} ===`);
  const result = spawnSync(
    process.execPath,
    [wdioBin, 'run', config, '--spec', spec],
    {
      cwd: root,
      env,
      stdio: 'inherit',
    },
  );

  if (result.error) throw result.error;
  if (result.status !== 0) {
    throw new Error(
      `Upgrade E2E phase ${phase} failed with exit code ${String(result.status)}.`,
    );
  }
}

try {
  console.log(`Using isolated upgrade runtime: ${runtime}`);
  runPhase('seed', true);
  runPhase('restart', false);
} finally {
  fs.rmSync(runtime, { recursive: true, force: true });
}
