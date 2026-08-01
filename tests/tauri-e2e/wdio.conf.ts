import fs from 'node:fs';
import path from 'node:path';
import { fileURLToPath } from 'node:url';

const configDirectory = path.dirname(fileURLToPath(import.meta.url));
const binaryName = process.platform === 'win32' ? 'chimera.exe' : 'chimera';
const appBinaryPath =
  process.env.CHIMERA_E2E_BINARY ??
  path.resolve(configDirectory, '../../backend/target/e2e/debug', binaryName);
const runtimeDirectory = path.resolve(configDirectory, '.tmp/runtime');
const configDirectoryOverride = path.join(runtimeDirectory, 'config');
const dataDirectoryOverride = path.join(runtimeDirectory, 'data');

fs.rmSync(runtimeDirectory, { force: true, recursive: true });
fs.mkdirSync(configDirectoryOverride, { recursive: true });
fs.mkdirSync(dataDirectoryOverride, { recursive: true });

export const config: WebdriverIO.Config = {
  runner: 'local',
  specs: ['./specs/**/*.e2e.ts'],
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
        driverProvider: 'embedded',
        embeddedPort: 4445,
        startTimeout: 120_000,
        statusPollTimeout: 10_000,
        captureBackendLogs: true,
        env: {
          CHIMERA_E2E_CONFIG_DIR: configDirectoryOverride,
          CHIMERA_E2E_DATA_DIR: dataDirectoryOverride,
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
};
