import assert from 'node:assert/strict';
import { execFileSync, spawn } from 'node:child_process';
import { once } from 'node:events';
import fs from 'node:fs';
import os from 'node:os';
import path from 'node:path';
import { setTimeout as delay } from 'node:timers/promises';
import { fileURLToPath } from 'node:url';
import { seedE2eDisabledAutoUpdate } from './e2e-update-preference.ts';
import {
  captureWindowsProxySettings,
  restoreWindowsProxySettings,
} from './process-cleanup.ts';

const here = path.dirname(fileURLToPath(import.meta.url));
const binary = path.resolve(here, '../backend/target/e2e/debug/chimera.exe');

function powershell(script: string): string {
  return execFileSync(
    'powershell.exe',
    ['-NoProfile', '-NonInteractive', '-Command', script],
    { encoding: 'utf8', timeout: 15_000, windowsHide: true },
  ).trim();
}

function appProcesses(): number[] {
  const json = powershell(
    "@(Get-CimInstance Win32_Process | Where-Object { $_.Name -ieq 'chimera.exe' } | Select-Object -ExpandProperty ProcessId) | ConvertTo-Json -Compress",
  );
  if (!json) return [];
  const parsed = JSON.parse(json) as number | number[];
  return Array.isArray(parsed) ? parsed : [parsed];
}

async function main(): Promise<void> {
  assert.equal(process.platform, 'win32', 'Native tray test requires Windows');
  assert.equal(
    process.env.CHIMERA_E2E_DEDICATED_VM,
    '1',
    'Use an isolated Windows VM with an interactive Explorer desktop',
  );
  assert.equal(
    process.env.CHIMERA_E2E_TRAY_QUIT,
    '1',
    'Explicitly opt into native tray testing',
  );
  assert.deepEqual(
    appProcesses(),
    [],
    'Close all other Chimera instances before testing',
  );
  assert.ok(fs.existsSync(binary), 'Build the E2E binary first');

  const runtime = fs.mkdtempSync(path.join(os.tmpdir(), 'chimera-tray-quit-'));
  const config = path.join(runtime, 'config');
  const data = path.join(runtime, 'data');
  fs.mkdirSync(config);
  fs.mkdirSync(data);
  seedE2eDisabledAutoUpdate(config);
  const snapshot = captureWindowsProxySettings();
  const logFile = path.join(runtime, 'app.log');
  const log = fs.openSync(logFile, 'w');
  const app = spawn(binary, [], {
    cwd: path.dirname(binary),
    env: {
      ...process.env,
      CHIMERA_E2E_CONFIG_DIR: config,
      CHIMERA_E2E_DATA_DIR: data,
    },
    stdio: ['ignore', log, log],
    windowsHide: false,
  });
  fs.closeSync(log);
  let exited = false;
  let passed = false;
  const result = once(app, 'exit').then(([code, signal]) => {
    exited = true;
    return { code, signal };
  });
  // Attach a rejection handler immediately; the assertion below still sees errors.
  void result.catch(() => {});
  try {
    assert.ok(app.pid, 'Failed to start the test application');
    // Run the native UI driver asynchronously so child exit events remain visible.
    const driver = spawn(
      'powershell.exe',
      [
        '-NoProfile',
        '-NonInteractive',
        '-File',
        path.join(here, 'windows-tray-quit.ps1'),
        '-AppPid',
        String(app.pid),
      ],
      { stdio: 'inherit', windowsHide: true },
    );
    let driverTimer: ReturnType<typeof setTimeout> | undefined;
    try {
      const [code] = await Promise.race([
        once(driver, 'exit'),
        new Promise<never>((_, reject) => {
          driverTimer = setTimeout(
            () => reject(new Error('Native tray interaction timed out')),
            80_000,
          );
        }),
      ]);
      assert.equal(code, 0, 'Native right-click / Quit interaction failed');
    } finally {
      clearTimeout(driverTimer);
      if (driver.exitCode === null) driver.kill();
    }

    // A disappearing menu or closed WebView is not enough: the original
    // application process must exit without a signal or forced termination.
    let timer: ReturnType<typeof setTimeout> | undefined;
    try {
      const actual = await Promise.race([
        result,
        new Promise<never>((_, reject) => {
          timer = setTimeout(
            () =>
              reject(
                new Error(
                  'Application still running 30 seconds after native Quit',
                ),
              ),
            30_000,
          );
        }),
      ]);
      assert.deepEqual(actual, { code: 0, signal: null });
    } finally {
      clearTimeout(timer);
    }
    await delay(250);
    assert.deepEqual(
      appProcesses(),
      [],
      'A Chimera application process remains after Quit',
    );
    // Check the application restored the system proxy BEFORE the harness
    // restores it in finally, so a cleanup bug cannot be masked by the test.
    assert.deepEqual(
      captureWindowsProxySettings(),
      snapshot,
      'System proxy was not restored by application shutdown',
    );
    passed = true;
    console.log(
      'Native tray Quit: application exited normally and host proxy was restored',
    );
  } finally {
    try {
      if (!exited && app.pid && app.exitCode === null) {
        app.kill();
        await Promise.race([result.catch(() => {}), delay(5000)]);
      }
    } finally {
      restoreWindowsProxySettings(snapshot);
      if (passed) fs.rmSync(runtime, { recursive: true, force: true });
      else console.error('Failure diagnostics retained at:', logFile);
    }
  }
}

await main();
