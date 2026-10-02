import { spawnSync } from 'node:child_process';
import { dirname, resolve } from 'node:path';
import { fileURLToPath } from 'node:url';

const repositoryRoot = resolve(dirname(fileURLToPath(import.meta.url)), '..');

if (process.platform !== 'darwin') {
  console.log(
    'Skipping the macOS DNS Proxy extension build on a non-macOS host.',
  );
  process.exit(0);
}

const buildScript = resolve(
  repositoryRoot,
  'backend/tauri/macos/dns-proxy/build-system-extension.sh',
);
const result = spawnSync('/bin/sh', [buildScript], {
  cwd: repositoryRoot,
  stdio: 'inherit',
});

if (result.error) {
  throw result.error;
}

process.exit(result.status ?? 1);
