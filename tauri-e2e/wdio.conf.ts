import fs from 'node:fs';
import path from 'node:path';
import { fileURLToPath } from 'node:url';
import {
  captureWindowsProxySettings,
  cleanupE2eProcesses,
  restoreWindowsProxySettings,
} from './process-cleanup.js';
import {
  cleanupRuntimeDirectory,
  pruneRuntimeDirectories,
  resolveRuntimeDirectory,
} from './runtime-path.js';

const configDirectory = path.dirname(fileURLToPath(import.meta.url));
const binaryName = process.platform === 'win32' ? 'chimera.exe' : 'chimera';
const appBinaryPath =
  process.env.CHIMERA_E2E_BINARY ??
  path.resolve(configDirectory, '../backend/target/e2e/debug', binaryName);
const runtimeRootDirectory = path.resolve(configDirectory, '.tmp/runtime');
const hostProxySnapshot =
  process.env.CHIMERA_E2E_SKIP_PROXY_RESTORE === '1'
    ? null
    : captureWindowsProxySettings();
const embeddedPort = Number(process.env.CHIMERA_E2E_WEBDRIVER_PORT ?? '4446');
const ownsRuntimeDirectory = !process.env.CHIMERA_E2E_RUNTIME_DIR;
const runtimeDirectory = resolveRuntimeDirectory(
  runtimeRootDirectory,
  process.env.CHIMERA_E2E_RUNTIME_DIR,
);

pruneRuntimeDirectories(runtimeRootDirectory, { exclude: [runtimeDirectory] });

if (ownsRuntimeDirectory) {
  process.env.CHIMERA_E2E_RUNTIME_DIR = runtimeDirectory;
}

const configDirectoryOverride = path.join(runtimeDirectory, 'config');
const dataDirectoryOverride = path.join(runtimeDirectory, 'data');

fs.mkdirSync(configDirectoryOverride, { recursive: true });
fs.mkdirSync(dataDirectoryOverride, { recursive: true });

type TauriBrowser = WebdriverIO.Browser & {
  tauri?: {
    restoreAllMocks: () => Promise<void>;
  };
};

async function prepareTauriServiceTeardown(): Promise<void> {
  if (browser.isMultiremote) return;

  const tauriBrowser = browser as TauriBrowser;
  if (tauriBrowser.sessionId) {
    await tauriBrowser.tauri?.restoreAllMocks();
  }

  const guardedExecute = async function (
    this: WebdriverIO.Browser,
    originalExecute: (...args: unknown[]) => Promise<unknown>,
    ...args: unknown[]
  ): Promise<unknown> {
    if (!this.sessionId) return undefined;
    return originalExecute(...args);
  };

  tauriBrowser.overwriteCommand(
    'execute',
    guardedExecute as Parameters<typeof tauriBrowser.overwriteCommand>[1],
  );
}

export const config: WebdriverIO.Config = {
  runner: 'local',
  specs: ['./specs/**/smoke.e2e.ts', './specs/**/proxy-localization.e2e.ts'],
  maxInstances: 1,
  logLevel: process.env.CI ? 'warn' : 'info',
  bail: 0,
  waitforTimeout: 30_000,
  connectionRetryTimeout: 120_000,
  connectionRetryCount: 1,
  services: [
    [
      '@wdio/tauri-service',
      {
        appBinaryPath,
        windowLabel: 'legacy',
        driverProvider: 'embedded',
        embeddedPort,
        startTimeout: 120_000,
        statusPollTimeout: 10_000,
        captureBackendLogs: true,
        env: {
          CHIMERA_E2E_CONFIG_DIR: configDirectoryOverride,
          CHIMERA_E2E_DATA_DIR: dataDirectoryOverride,
          TAURI_WEBDRIVER_PORT: String(embeddedPort),
        },
      },
    ],
  ],
  capabilities: [
    {
      browserName: 'tauri',
    },
  ],
  framework: 'mocha',
  reporters: ['spec'],
  mochaOpts: {
    ui: 'bdd',
    timeout: 120_000,
  },
  after: prepareTauriServiceTeardown,
  onComplete: async () => {
    try {
      await cleanupE2eProcesses(
        path.dirname(appBinaryPath),
        runtimeRootDirectory,
      );
    } finally {
      try {
        if (ownsRuntimeDirectory) {
          cleanupRuntimeDirectory(runtimeRootDirectory, runtimeDirectory);
        }
      } finally {
        restoreWindowsProxySettings(hostProxySnapshot);
      }
    }
  },
};
