import { readFile } from 'node:fs/promises';
import * as path from 'node:path';
import { parseArgs } from 'node:util';
import { runCommand } from '../shared/command.ts';
import { colorize, consola } from '../shared/logger.ts';
import { WORKSPACE_ROOT } from '../shared/repo-paths.ts';
import { createBinaryResolvers } from './binaries.ts';
import { normalizeArch, normalizePlatform } from './platform.ts';
import {
  formatProgressSize,
  formatResolveInfo,
  formatSpeed,
  ProgressRenderer,
} from './progress.ts';
import { prepareRetryDelayMs } from './retry.ts';
import { createPrepareTasks, selectPrepareTasks, type Task } from './tasks.ts';
import type { VersionManifest } from './types.ts';
import { normalizeVersion } from './versions.ts';

interface CheckArgs {
  force?: boolean;
  arch?: string;
  'sidecar-host'?: string;
}

async function runTask(
  queue: Task[],
  progress: ProgressRenderer,
): Promise<void> {
  const task = queue.shift();
  if (!task) return;

  for (let attempt = 0; attempt < task.retry; attempt++) {
    try {
      progress.update(
        task.name,
        'Pulling',
        task.version ? { version: task.version } : {},
      );
      const info = await task.func((download) => {
        progress.update(task.name, 'Pulling', {
          version: normalizeVersion(download.version) ?? task.version,
          size: formatProgressSize(download.downloaded, download.total),
          speed: formatSpeed(download.speed),
        });
      });
      progress.update(task.name, 'Done', formatResolveInfo(info));
      break;
    } catch (err) {
      if (attempt === task.retry - 1) {
        progress.update(task.name, 'Failed');
        progress.finish();
        consola.fatal(`task::${task.name} failed`, err);
        process.exit(1);
      }
      progress.update(task.name, 'Retrying', {
        note: `${attempt + 1}/${task.retry}`,
        version: task.version,
      });
      const delayMs = prepareRetryDelayMs(err, attempt, task.name);
      if (delayMs) {
        await new Promise((resolve) => setTimeout(resolve, delayMs));
      }
    }
  }

  return runTask(queue, progress);
}

async function resolveSidecarHost(
  hostOverride?: string,
): Promise<string | undefined> {
  if (hostOverride) return hostOverride;
  const { success, stdout } = await runCommand('rustc', { args: ['-vV'] });
  if (!success) return undefined;
  const output = stdout.toString('utf8');
  return output.match(/host: (.+)/)?.[1]?.trim();
}

export async function main(
  args = parseArgs({
    args: process.argv.slice(2),
    options: {
      force: { type: 'boolean' },
      arch: { type: 'string' },
      'sidecar-host': { type: 'string' },
    },
    strict: false,
  }).values as CheckArgs,
): Promise<void> {
  const force = args.force;
  const arch = args.arch ?? normalizeArch(process.arch);
  const platform = normalizePlatform(process.platform);
  const debug = process.env.LOG_LEVEL !== undefined;
  const debugLog = (message: string) => {
    if (debug) consola.debug(message);
  };

  const sidecarHost = await resolveSidecarHost(args['sidecar-host']);
  if (!sidecarHost) {
    consola.fatal(colorize`{red.bold SIDECAR_HOST} not found`);
    process.exit(1);
  }

  debugLog(colorize`sidecar-host {yellow ${sidecarHost}}`);
  debugLog(colorize`platform {yellow ${platform}}`);
  debugLog(colorize`arch {yellow ${arch}}`);

  const versionManifest = JSON.parse(
    await readFile(path.join(WORKSPACE_ROOT, 'manifest/version.json'), 'utf8'),
  ) as VersionManifest;
  const binary = createBinaryResolvers({
    versionManifest,
    sidecarHost,
    platform,
    arch,
    workspaceRoot: WORKSPACE_ROOT,
    debugLog,
  });
  const resourceContext = {
    resourcesDir: path.join(WORKSPACE_ROOT, 'backend/tauri/resources'),
    sidecarDir: path.join(WORKSPACE_ROOT, 'backend/tauri/sidecar'),
    tempRoot: path.join(WORKSPACE_ROOT, 'node_modules/.verge'),
    platform,
    arch,
    debugLog,
  };
  const tasks = createPrepareTasks({
    force,
    versionManifest,
    binary,
    resourceContext,
  });
  const activeTasks = selectPrepareTasks(tasks, platform);
  const progress = new ProgressRenderer(
    activeTasks.map((task) => task.name),
    consola,
    process.stdout.isTTY && !process.env.CI && !debug,
    (text) => process.stdout.write(text),
  );
  progress.start();

  const concurrency = Math.ceil(navigator.hardwareConcurrency / 2) || 2;
  const queue = [...activeTasks];
  await Promise.all(
    Array.from({ length: concurrency }, () => runTask(queue, progress)),
  );
  progress.finish();

  consola.log('Chimera');
  consola.success('all resources download finished\n');
  consola.log('  next command:\n');
  consola.log('    pnpm dev - development');
  consola.log('    pnpm dev:diff - development with deadlock detection');
}

if (import.meta.main) await main();
