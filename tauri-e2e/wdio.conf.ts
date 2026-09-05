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
import { e2eSuites } from './spec-suites.js';

const configDirectory = path.dirname(fileURLToPath(import.meta.url));
const selectedSuiteIndex = process.argv.indexOf('--suite');
const selectedSuite =
  selectedSuiteIndex >= 0 ? process.argv[selectedSuiteIndex + 1] : undefined;
const agentFixtureSuites = new Set(['agent', 'hermetic', 'all']);
const agentFixture =
  process.env.CHIMERA_E2E_AGENT_FIXTURE ??
  (selectedSuite && agentFixtureSuites.has(selectedSuite)
    ? 'stale-proxy'
    : undefined);
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
const artifactDirectory = process.env.CHIMERA_E2E_ARTIFACT_DIR
  ? path.resolve(process.env.CHIMERA_E2E_ARTIFACT_DIR)
  : null;

fs.mkdirSync(configDirectoryOverride, { recursive: true });
fs.mkdirSync(dataDirectoryOverride, { recursive: true });

const configFixtureDirectory = process.env.CHIMERA_E2E_CONFIG_FIXTURE
  ? path.resolve(process.env.CHIMERA_E2E_CONFIG_FIXTURE)
  : null;
const configFixtureMarker = path.join(
  runtimeDirectory,
  '.config-fixture-seeded',
);

if (configFixtureDirectory && !fs.existsSync(configFixtureMarker)) {
  const fixture = fs.statSync(configFixtureDirectory);
  if (!fixture.isDirectory()) {
    throw new Error(
      `CHIMERA_E2E_CONFIG_FIXTURE is not a directory: ${configFixtureDirectory}`,
    );
  }

  const existing = fs.readdirSync(configDirectoryOverride);
  if (existing.length > 0) {
    throw new Error(
      `Refusing to seed a non-empty E2E config directory: ${configDirectoryOverride}`,
    );
  }

  for (const entry of fs.readdirSync(configFixtureDirectory)) {
    fs.cpSync(
      path.join(configFixtureDirectory, entry),
      path.join(configDirectoryOverride, entry),
      { recursive: true },
    );
  }
  fs.writeFileSync(configFixtureMarker, configFixtureDirectory);
}

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

function sanitizeArtifactPart(value: string): string {
  return value.replace(/[^a-z0-9._-]+/gi, '-').replace(/^-+|-+$/g, '');
}

async function captureFailedTest(
  test: { parent?: string; title?: string },
  result: { passed: boolean; error?: unknown },
): Promise<void> {
  if (!artifactDirectory || result.passed || !browser.sessionId) return;

  const name = [test.parent, test.title]
    .filter((part): part is string => Boolean(part))
    .map(sanitizeArtifactPart)
    .filter(Boolean)
    .join('-');
  const artifactBase = path.join(
    artifactDirectory,
    name || `failed-test-${process.pid}`,
  );

  fs.mkdirSync(artifactDirectory, { recursive: true });
  await browser.saveScreenshot(`${artifactBase}.png`).catch(() => undefined);
  await browser
    .getPageSource()
    .then((source) => fs.writeFileSync(`${artifactBase}.html`, source))
    .catch(() => undefined);
  if (result.error) {
    fs.writeFileSync(`${artifactBase}.error.txt`, String(result.error));
  }
}

export const config: WebdriverIO.Config = {
  runner: 'local',
  specs: [...e2eSuites.smoke],
  suites: Object.fromEntries(
    Object.entries(e2eSuites).map(([name, specs]) => [name, [...specs]]),
  ),
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
          ...(agentFixture ? { CHIMERA_E2E_AGENT_FIXTURE: agentFixture } : {}),
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
  outputDir: artifactDirectory ?? undefined,
  mochaOpts: {
    ui: 'bdd',
    timeout: 120_000,
  },
  afterTest: captureFailedTest,
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
