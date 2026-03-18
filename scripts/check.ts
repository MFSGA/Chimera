import os from 'node:os';
import path from 'node:path';
import { ensureDir, exists } from 'fs-extra';
import { archCheck } from './utils/arch-check';
import { SIDECAR_HOST } from './utils/consts';
import { downloadFile } from './utils/download';
import { TAURI_APP_DIR } from './utils/env';
import { colorize, consola } from './utils/logger';
import { Resolve } from './utils/resolve';

// force download
const FORCE = process.argv.includes('--force');
console.log(FORCE);
console.log(FORCE);

// cross platform build using
const ARCH = process.argv.includes('--arch')
  ? process.argv[process.argv.indexOf('--arch') + 1]
  : undefined;

// cross platform build support
if (!SIDECAR_HOST) {
  consola.fatal(colorize`{red.bold SIDECAR_HOST} not found`);

  process.exit(1);
} else {
  consola.debug(colorize`sidecar-host {yellow ${SIDECAR_HOST}}`);
}

const platform = process.platform;

const arch = (ARCH || process.arch) as NodeJS.Architecture | 'armel';

console.log(platform);

archCheck(platform, arch);

const resolve = new Resolve({
  platform,
  arch,
  sidecarHost: SIDECAR_HOST!,
  // todo: should delete in the future
  force: FORCE,
});

const tasks: {
  name: string;
  func: () => Promise<void>;
  retry: number;
  winOnly?: boolean;
}[] = [
  { name: 'mihomo', func: () => resolve.clashMeta(), retry: 5 },
  // { name: 'mihomo-alpha', func: () => resolve.clashMetaAlpha(), retry: 5 },
  { name: 'clash-rs', func: () => resolve.clashRust(), retry: 5 },
  { name: 'chimera-client', func: () => resolve.chimeraClient(), retry: 5 },
  { name: 'clash-rs-alpha', func: () => resolve.clashRustAlpha(), retry: 5 },
  { name: 'nyanpasu-service', func: () => resolve.service(), retry: 5 },
  { name: 'chimera-service', func: () => resolve.chimeraService(), retry: 5 },

  { name: 'mmdb', func: () => resolve.mmdb(), retry: 5 },
  { name: 'geoip', func: () => resolve.geoip(), retry: 5 },
  { name: 'geosite', func: () => resolve.geosite(), retry: 5 },
  { name: 'wintun', func: () => resolve.wintun(), retry: 5, winOnly: true },
  {
    name: 'enableLoopback',
    func: () =>
      resolveResource(
        {
          file: 'enableLoopback.exe',
          downloadURL:
            'https://github.com/Kuingsmile/uwp-tool/releases/download/latest/enableLoopback.exe',
        },
        { force: FORCE },
      ),
    retry: 5,
    winOnly: true,
  },
];

async function runTask() {
  const task = tasks.shift();

  if (!task) return;

  if (task.winOnly && process.platform !== 'win32') return runTask();

  for (let i = 0; i < task.retry; i++) {
    try {
      await task.func();

      break;
    } catch (err) {
      consola.warn(`task::${task.name} try ${i} ==`, err);

      if (i === task.retry - 1) {
        consola.fatal(`task::${task.name} failed`, err);
        process.exit(1);
      }
    }
  }

  return runTask();
}

consola.start('start check and download resources...');

const jobs = new Array(Math.ceil(os.cpus.length / 2) || 2)
  .fill(0)
  .map(() => runTask());

Promise.all(jobs).then(() => {
  // todo: printNyanpasu()

  consola.success('all resources download finished\n');

  const commands = [
    'pnpm dev - development with react dev tools',
    'pnpm dev:diff - deadlock development with react dev tools (recommend)',
    'pnpm tauri:diff - deadlock development',
  ];

  consola.log('  next command:\n');

  commands.forEach((text) => {
    consola.log(`    ${text}`);
  });
});

// === Resource Resolution ===

async function resolveResource(
  binInfo: { file: string; downloadURL: string },
  options?: { force?: boolean },
): Promise<void> {
  const { file, downloadURL } = binInfo;
  const resDir = path.join(TAURI_APP_DIR, 'resources');
  const targetPath = path.join(resDir, file);

  if (!options?.force && (await exists(targetPath))) return;

  await ensureDir(resDir);
  await downloadFile(downloadURL, targetPath);

  consola.success(colorize`resolve {green ${file}} finished`);
}
