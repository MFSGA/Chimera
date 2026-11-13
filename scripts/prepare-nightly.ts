import { readdir, readFile, writeFile } from 'node:fs/promises';
import * as path from 'node:path';
import { runCommand } from './src/shared/command.ts';
import { consola } from './src/shared/logger.ts';

type JsonObject = Record<string, any>;

function mergeJson(base: JsonObject, override: JsonObject): JsonObject {
  // Match the existing lodash merge behavior for JSON objects and arrays,
  // without introducing a new runtime dependency or touching pnpm-lock.yaml.
  for (const [key, value] of Object.entries(override)) {
    const previous = base[key];
    if (Array.isArray(value)) {
      const values = Array.isArray(previous) ? [...previous] : [];
      for (let i = 0; i < value.length; i++) {
        values[i] =
          value[i] && typeof value[i] === 'object' && !Array.isArray(value[i])
            ? mergeJson(
                previous?.[i] && typeof previous[i] === 'object'
                  ? { ...previous[i] }
                  : {},
                value[i],
              )
            : value[i];
      }
      base[key] = values;
    } else if (value && typeof value === 'object') {
      base[key] = mergeJson(
        previous && typeof previous === 'object' && !Array.isArray(previous)
          ? { ...previous }
          : {},
        value,
      );
    } else {
      base[key] = value;
    }
  }
  return base;
}

async function readJson(file: string): Promise<JsonObject> {
  return JSON.parse(await readFile(file, 'utf8')) as JsonObject;
}

async function fixedWebviewPath(tauriDir: string): Promise<string> {
  const entry = (await readdir(tauriDir)).find((name) =>
    name.includes('WebView2'),
  );
  if (!entry) throw new Error('WebView2 runtime not found');
  return `./${path.basename(entry)}`;
}

/**
 * Prepare the nightly configuration in one workspace.
 * The cwd override exists for disposable test repositories only.
 */
export async function prepareNightly(
  cwd = process.cwd(),
  args: readonly string[] = process.argv.slice(2),
): Promise<void> {
  const tauriDir = path.join(cwd, 'backend/tauri');
  const tauriConf = mergeJson(
    await readJson(path.join(tauriDir, 'tauri.conf.json')),
    await readJson(path.join(tauriDir, 'overrides/nightly.conf.json')),
  );
  const chimeraPath = path.join(cwd, 'frontend/chimera/package.json');
  const rootPath = path.join(cwd, 'package.json');
  const chimeraPackage = await readJson(chimeraPath);
  const rootPackage = await readJson(rootPath);

  const isNSIS = args.includes('--nsis');
  const isMSI = args.includes('--msi');
  const isLinux = args.includes('--linux');
  const isFixedWebview = args.includes('--fixed-webview');
  const disableUpdater = args.includes('--disable-updater');

  if (isFixedWebview) {
    mergeJson(
      tauriConf,
      await readJson(path.join(tauriDir, 'overrides/fixed-webview2.conf.json')),
    );
    const webviewMode = tauriConf.bundle.windows.webviewInstallMode;
    delete webviewMode.silent;
    webviewMode.path = await fixedWebviewPath(tauriDir);
    // Preserve Chimera's separate fixed-WebView nightly updater endpoints.
    tauriConf.plugins.updater.endpoints =
      tauriConf.plugins.updater.endpoints.map((endpoint: string) =>
        endpoint.replace('update-', 'update-nightly-'),
      );
  }

  if (isNSIS) tauriConf.bundle.targets = ['nsis'];
  if (disableUpdater) tauriConf.bundle.createUpdaterArtifacts = false;

  const git = await runCommand('git', {
    cwd,
    args: ['rev-parse', 'HEAD'],
  });
  if (!git.success) {
    throw new Error(
      `failed to resolve nightly Git commit: ${git.stderr.toString('utf8')}`,
    );
  }
  const hash = git.stdout.toString('utf8').trim().slice(0, 7);
  if (!/^[0-9a-f]{7}$/.test(hash))
    throw new Error('invalid nightly commit hash');
  const baseVersion = tauriConf.version;
  const nightlyVersion = `${baseVersion}-alpha+${hash}`;
  const linuxVersion = `${baseVersion}+alpha.${hash}`;

  // Keep the existing RPM/DEB and Tauri/NSIS/MSI version semantics.
  if (isLinux) tauriConf.version = linuxVersion;
  else if (!isNSIS && !isMSI) tauriConf.version = nightlyVersion;

  chimeraPackage.version = nightlyVersion;
  rootPackage.version = nightlyVersion;
  await writeFile(
    path.join(tauriDir, 'tauri.nightly.conf.json'),
    JSON.stringify(tauriConf, null, 2),
  );
  await writeFile(chimeraPath, JSON.stringify(chimeraPackage, null, 2));
  await writeFile(rootPath, JSON.stringify(rootPackage, null, 2));
  consola.success(`Nightly configuration prepared for ${hash}`);
}

if (import.meta.main) {
  await prepareNightly();
}
