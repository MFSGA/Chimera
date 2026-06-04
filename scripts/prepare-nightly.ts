import * as path from 'jsr:@std/path';
import { merge } from 'npm:lodash-es';
import { consola } from './deno/utils/logger.ts';

const cwd = Deno.cwd();
const TAURI_APP_DIR = path.join(cwd, 'backend/tauri');
const TAURI_FIXED_WEBVIEW2_CONFIG_OVERRIDE_PATH = path.join(
  TAURI_APP_DIR,
  'overrides/fixed-webview2.conf.json',
);
const TAURI_DEV_APP_CONF_PATH = path.join(
  TAURI_APP_DIR,
  'tauri.nightly.conf.json',
);
const TAURI_APP_CONF = path.join(TAURI_APP_DIR, 'tauri.conf.json');
const TAURI_DEV_APP_OVERRIDES_PATH = path.join(
  TAURI_APP_DIR,
  'overrides/nightly.conf.json',
);
const CHIMERA_PACKAGE_JSON_PATH = path.join(
  cwd,
  'frontend/chimera/package.json',
);
const ROOT_PACKAGE_JSON_PATH = path.join(cwd, 'package.json');

const isNSIS = Deno.args.includes('--nsis');
const isMSI = Deno.args.includes('--msi');
const fixedWebview = Deno.args.includes('--fixed-webview');
const disableUpdater = Deno.args.includes('--disable-updater');

async function readJsonFile<T>(filePath: string): Promise<T> {
  return JSON.parse(await Deno.readTextFile(filePath)) as T;
}

async function writeJsonFile(filePath: string, value: unknown) {
  await Deno.writeTextFile(filePath, JSON.stringify(value, null, 2));
}

async function resolveFixedWebviewPath() {
  for await (const entry of Deno.readDir(TAURI_APP_DIR)) {
    if (entry.name.includes('WebView2')) {
      return `./${path.basename(entry.name)}`;
    }
  }

  throw new Error('WebView2 runtime not found');
}

async function getCurrentGitShortHash() {
  const gitResult = await new Deno.Command('git', {
    args: ['rev-parse', '--short', 'HEAD'],
    stdout: 'piped',
  }).output();
  return new TextDecoder().decode(gitResult.stdout).trim();
}

async function main() {
  consola.debug('Read config...');
  const tauriAppConf =
    await readJsonFile<Record<string, unknown>>(TAURI_APP_CONF);
  const tauriAppOverrides = await readJsonFile<Record<string, unknown>>(
    TAURI_DEV_APP_OVERRIDES_PATH,
  );
  let tauriConf = merge(tauriAppConf, tauriAppOverrides);
  const chimeraPackageJson = await readJsonFile<Record<string, unknown>>(
    CHIMERA_PACKAGE_JSON_PATH,
  );
  const rootPackageJson = await readJsonFile<Record<string, unknown>>(
    ROOT_PACKAGE_JSON_PATH,
  );

  if (fixedWebview) {
    const fixedWebview2Config = await readJsonFile<Record<string, unknown>>(
      TAURI_FIXED_WEBVIEW2_CONFIG_OVERRIDE_PATH,
    );
    tauriConf = merge(tauriConf, fixedWebview2Config);
    const webviewInstallMode = tauriConf.bundle.windows.webviewInstallMode;
    delete webviewInstallMode.silent;
    webviewInstallMode.path = await resolveFixedWebviewPath();
    tauriConf.plugins.updater.endpoints =
      tauriConf.plugins.updater.endpoints.map((endpoint: string) =>
        endpoint.replace('update-', 'update-nightly-'),
      );
  }

  if (isNSIS) {
    tauriConf.bundle.targets = ['nsis'];
  }

  if (disableUpdater) {
    tauriConf.bundle.createUpdaterArtifacts = false;
  }

  consola.debug('Get current git short hash');
  const gitShortHash = await getCurrentGitShortHash();
  consola.debug(`Current git short hash: ${gitShortHash}`);

  const version = `${tauriConf.version}-alpha+${gitShortHash}`;

  consola.debug('Write tauri version to tauri.nightly.conf.json');
  if (!isNSIS && !isMSI) tauriConf.version = version;
  await writeJsonFile(TAURI_DEV_APP_CONF_PATH, tauriConf);
  consola.debug('tauri.nightly.conf.json updated');

  consola.debug('Write nightly version to package.json');
  chimeraPackageJson.version = version;
  await writeJsonFile(CHIMERA_PACKAGE_JSON_PATH, chimeraPackageJson);
  rootPackageJson.version = version;
  await writeJsonFile(ROOT_PACKAGE_JSON_PATH, rootPackageJson);
  consola.debug('package.json updated');
}

main();
