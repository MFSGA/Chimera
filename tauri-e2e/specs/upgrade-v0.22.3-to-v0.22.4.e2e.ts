import assert from 'node:assert/strict';
import fs from 'node:fs';
import path from 'node:path';

interface WindowState {
  width: number;
  height: number;
  x: number;
  y: number;
  maximized: boolean;
  fullscreen: boolean;
}

interface VergeConfig {
  enable_auto_check_update?: boolean;
  enable_auto_launch?: boolean;
  enable_system_proxy?: boolean;
  enable_service_mode?: boolean;
  always_on_top?: boolean;
  max_log_files?: number;
  theme_mode?: string;
  language?: string;
  clash_core?: string;
  system_proxy_bypass?: string;
  proxy_guard_interval?: number;
  break_when_proxy_change?: 'none' | 'chain' | 'all';
  break_when_profile_change?: boolean;
  break_when_mode_change?: boolean;
  window_type?: 'legacy' | 'main';
  window_size_state?: WindowState;
}

interface ClashInfo {
  secret?: string;
  server: string;
}

interface RuntimeConfig {
  ipv6: boolean;
  'allow-lan': boolean;
  mode: string;
}

type UpgradePhase = 'seed' | 'restart';

const upgradePhase = process.env.CHIMERA_E2E_UPGRADE_PHASE as
  UpgradePhase | undefined;
const describeUpgrade = upgradePhase ? describe : describe.skip;

async function waitForApp(): Promise<void> {
  await browser.waitUntil(
    async () =>
      browser.execute(
        () => (document.getElementById('root')?.childElementCount ?? 0) > 0,
      ),
    { timeout: 30_000, timeoutMsg: 'The Chimera frontend did not render.' },
  );
}

async function invoke<T>(
  command: string,
  args?: Record<string, unknown>,
): Promise<T> {
  return browser.execute(
    async (name, payload) => {
      const tauri = (
        window as typeof window & {
          __TAURI_INTERNALS__: {
            invoke: (
              command: string,
              args?: Record<string, unknown>,
            ) => Promise<T>;
          };
        }
      ).__TAURI_INTERNALS__;
      return tauri.invoke(name, payload);
    },
    command,
    args,
  );
}

function configDirectory(): string {
  const runtime = process.env.CHIMERA_E2E_RUNTIME_DIR;
  assert.ok(
    runtime,
    'CHIMERA_E2E_RUNTIME_DIR must be set by the upgrade harness.',
  );
  return path.join(runtime, 'config');
}

function typedPath(name: string): string {
  return path.join(configDirectory(), name);
}

function persistedBoolean(file: string, key: string): boolean | null {
  if (!fs.existsSync(file)) return null;
  const text = fs.readFileSync(file, 'utf8');
  const pattern = new RegExp(`^\\s*${key}:\\s*(true|false)\\s*$`, 'm');
  const match = pattern.exec(text);
  return match ? match[1] === 'true' : null;
}

function persistedMainWindowState(file: string): WindowState | null {
  if (!fs.existsSync(file)) return null;

  const lines = fs.readFileSync(file, 'utf8').split(/\r?\n/);
  const mainIndex = lines.findIndex((line) => /^\s+main:\s*$/.test(line));
  if (mainIndex < 0) return null;

  const mainIndent = lines[mainIndex].match(/^\s*/)?.[0].length ?? 0;
  const values = new Map<string, string>();
  for (let index = mainIndex + 1; index < lines.length; index += 1) {
    const line = lines[index];
    if (!line.trim()) continue;
    const indent = line.match(/^\s*/)?.[0].length ?? 0;
    if (indent <= mainIndent) break;
    const match =
      /^\s+(width|height|x|y|maximized|fullscreen):\s*(.+)\s*$/.exec(line);
    if (match) values.set(match[1], match[2]);
  }

  const width = Number(values.get('width'));
  const height = Number(values.get('height'));
  const x = Number(values.get('x'));
  const y = Number(values.get('y'));
  const maximized = values.get('maximized');
  const fullscreen = values.get('fullscreen');
  if (
    !Number.isFinite(width) ||
    !Number.isFinite(height) ||
    !Number.isFinite(x) ||
    !Number.isFinite(y) ||
    !maximized ||
    !fullscreen
  ) {
    return null;
  }

  return {
    width,
    height,
    x,
    y,
    maximized: maximized === 'true',
    fullscreen: fullscreen === 'true',
  };
}

async function readVergeConfig(): Promise<VergeConfig> {
  return invoke<VergeConfig>('get_verge_config');
}

async function readRuntimeConfig(): Promise<RuntimeConfig> {
  const info = await invoke<ClashInfo>('get_clash_info');
  const response = await fetch(`http://${info.server}/configs`, {
    headers: info.secret
      ? { Authorization: `Bearer ${info.secret}` }
      : undefined,
  });
  if (!response.ok) {
    throw new Error(`Clash config query failed: ${response.status}`);
  }
  return (await response.json()) as RuntimeConfig;
}

async function waitForRuntimeConfig(): Promise<RuntimeConfig> {
  let config: RuntimeConfig | undefined;
  await browser.waitUntil(
    async () => {
      try {
        config = await readRuntimeConfig();
        return true;
      } catch {
        return false;
      }
    },
    {
      timeout: 30_000,
      timeoutMsg: 'The upgraded Clash core did not expose /configs.',
    },
  );
  assert.ok(config);
  return config;
}

function assertLegacySeed(config: VergeConfig): void {
  assert.equal(config.enable_auto_check_update, false);
  assert.equal(config.enable_auto_launch, true);
  assert.equal(config.enable_system_proxy, false);
  assert.equal(config.enable_service_mode, false);
  assert.equal(config.always_on_top, true);
  assert.equal(config.max_log_files, 11);
  assert.equal(config.theme_mode, 'dark');
  assert.equal(config.language, 'en-US');
  assert.equal(config.clash_core, 'mihomo');
  assert.equal(config.system_proxy_bypass, 'localhost;127.*;10.*');
  assert.equal(config.proxy_guard_interval, 17);
  assert.equal(config.break_when_proxy_change, 'chain');
  assert.equal(config.break_when_profile_change, true);
  assert.equal(config.break_when_mode_change, false);
  assert.equal(config.window_type, 'legacy');
  assert.equal(config.window_size_state?.width, 820);
  assert.equal(config.window_size_state?.height, 620);
}

async function waitForTypedCommit(): Promise<WindowState> {
  const application = typedPath('application.yaml');
  const clash = typedPath('clash-config.yaml');
  const session = typedPath('session-state.yaml');
  let persistedWindow: WindowState | null = null;

  await browser.waitUntil(
    async () => {
      persistedWindow = persistedMainWindowState(session);
      const legacy = await readVergeConfig();
      const runtime = await readRuntimeConfig().catch(() => null);
      return Boolean(
        persistedBoolean(application, 'enable_auto_check_update') === true &&
        persistedBoolean(application, 'enable_auto_launch') === true &&
        persistedBoolean(clash, 'ipv6') === false &&
        persistedWindow &&
        legacy.enable_auto_check_update === true &&
        legacy.window_size_state &&
        runtime?.ipv6 === false &&
        legacy.window_size_state.width === persistedWindow.width &&
        legacy.window_size_state.height === persistedWindow.height &&
        legacy.window_size_state.x === persistedWindow.x &&
        legacy.window_size_state.y === persistedWindow.y,
      );
    },
    {
      timeout: 30_000,
      timeoutMsg:
        'Typed Application/Clash/Session state did not commit and mirror after the upgrade seed.',
    },
  );

  assert.ok(persistedWindow);
  return persistedWindow;
}

async function runSeedPhase(): Promise<void> {
  const application = typedPath('application.yaml');
  const clash = typedPath('clash-config.yaml');
  const session = typedPath('session-state.yaml');

  assertLegacySeed(await readVergeConfig());
  const runtimeBefore = await waitForRuntimeConfig();
  assert.equal(runtimeBefore.ipv6, true);
  assert.equal(runtimeBefore['allow-lan'], true);
  assert.equal(runtimeBefore.mode.toLowerCase(), 'global');

  assert.equal(fs.existsSync(application), false);
  assert.equal(fs.existsSync(clash), true);
  assert.equal(persistedBoolean(clash, 'ipv6'), true);
  assert.equal(fs.existsSync(session), false);

  await invoke<null>('patch_verge_config', {
    payload: { enable_auto_check_update: true },
  });
  await invoke<null>('patch_clash_config', { payload: { ipv6: false } });

  const original = await browser.getWindowSize();
  await browser.setWindowSize(
    Math.max(860, original.width + 40),
    Math.max(680, original.height + 30),
  );
  await invoke<null>('save_window_size_state', { label: 'legacy' });

  const persistedWindow = await waitForTypedCommit();
  assert.ok(persistedWindow.width > 0);
  assert.ok(persistedWindow.height > 0);
}

async function runRestartPhase(): Promise<void> {
  const application = typedPath('application.yaml');
  const clash = typedPath('clash-config.yaml');
  const session = typedPath('session-state.yaml');

  assert.equal(fs.existsSync(application), true);
  assert.equal(fs.existsSync(clash), true);
  assert.equal(fs.existsSync(session), true);

  const legacy = await readVergeConfig();
  assert.equal(legacy.enable_auto_check_update, true);
  assert.equal(legacy.enable_auto_launch, true);
  assert.equal(legacy.enable_system_proxy, false);
  assert.equal(legacy.enable_service_mode, false);
  assert.equal(legacy.always_on_top, true);
  assert.equal(legacy.max_log_files, 11);
  assert.equal(legacy.theme_mode, 'dark');
  assert.equal(legacy.language, 'en-US');
  assert.equal(legacy.clash_core, 'mihomo');
  assert.equal(legacy.system_proxy_bypass, 'localhost;127.*;10.*');
  assert.equal(legacy.proxy_guard_interval, 17);
  assert.equal(legacy.break_when_proxy_change, 'chain');
  assert.equal(legacy.break_when_profile_change, true);
  assert.equal(legacy.break_when_mode_change, false);
  assert.equal(legacy.window_type, 'legacy');

  assert.equal(persistedBoolean(application, 'enable_auto_check_update'), true);
  assert.equal(persistedBoolean(application, 'enable_auto_launch'), true);
  assert.equal(persistedBoolean(clash, 'ipv6'), false);

  const runtime = await waitForRuntimeConfig();
  assert.equal(runtime.ipv6, false);
  assert.equal(runtime['allow-lan'], true);
  assert.equal(runtime.mode.toLowerCase(), 'global');

  const persistedWindow = persistedMainWindowState(session);
  assert.ok(persistedWindow);
  assert.deepEqual(legacy.window_size_state, persistedWindow);
}

describeUpgrade('Windows v0.22.3 to v0.22.4 typed-state upgrade', () => {
  it('preserves legacy settings, commits typed state, and survives a real process restart', async () => {
    await waitForApp();
    assert.ok(
      upgradePhase === 'seed' || upgradePhase === 'restart',
      'CHIMERA_E2E_UPGRADE_PHASE must be seed or restart.',
    );

    if (upgradePhase === 'seed') {
      await runSeedPhase();
    } else {
      await runRestartPhase();
    }
  });
});
