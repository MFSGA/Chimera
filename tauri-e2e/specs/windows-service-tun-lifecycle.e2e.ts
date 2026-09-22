
import assert from 'node:assert/strict';
import { execFile } from 'node:child_process';
import fs from 'node:fs';
import path from 'node:path';
import { fileURLToPath } from 'node:url';
import { promisify } from 'node:util';

const execFileAsync = promisify(execFile);
const specDirectory = path.dirname(fileURLToPath(import.meta.url));
const repoRoot = path.resolve(specDirectory, '../..');
const serviceBinary =
  process.env.CHIMERA_E2E_SERVICE_BINARY ??
  path.join(repoRoot, 'backend', 'target', 'e2e', 'debug', 'chimera-service.exe');
const tunSmokeScript = path.join(repoRoot, 'scripts', 'windows-tun-smoke.ps1');

type CoreState = 'Running' | { Stopped: string | null };
type RunType = 'normal' | 'service' | 'elevated';
type ServiceStatus = 'not_installed' | 'stopped' | 'running';

interface VergeConfig {
  enable_tun_mode?: boolean | null;
  enable_service_mode?: boolean | null;
}

interface ServiceStatusInfo {
  status: ServiceStatus;
  phase: string;
  runtime_owned: boolean;
}

interface TunSmokeReport {
  passed: boolean;
  checks: Array<{ name: string; passed: boolean; detail: string }>;
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

async function isElevated(): Promise<boolean> {
  if (process.platform !== 'win32') return false;
  try {
    await execFileAsync('powershell.exe', [
      '-NoProfile',
      '-NonInteractive',
      '-Command',
      [
        '$identity=[Security.Principal.WindowsIdentity]::GetCurrent()',
        '$principal=New-Object Security.Principal.WindowsPrincipal($identity)',
        'if ($principal.IsInRole([Security.Principal.WindowsBuiltInRole]::Administrator)) { exit 0 }',
        'exit 1',
      ].join('; '),
    ]);
    return true;
  } catch {
    return false;
  }
}

async function readServiceCliStatus(): Promise<{ status: ServiceStatus }> {
  const result = await execFileAsync(serviceBinary, ['status', '--json'], {
    timeout: 10_000,
    windowsHide: true,
  });
  return JSON.parse(result.stdout) as { status: ServiceStatus };
}

async function waitForService(
  predicate: (status: ServiceStatusInfo) => boolean,
  description: string,
): Promise<ServiceStatusInfo> {
  let last: ServiceStatusInfo | null = null;
  await browser.waitUntil(
    async () => {
      last = await invoke<ServiceStatusInfo>('status_service');
      return predicate(last);
    },
    {
      timeout: 45_000,
      interval: 250,
      timeoutMsg: description,
    },
  );
  assert.ok(last, description);
  return last;
}

async function waitForCore(
  predicate: (state: CoreState, runType: RunType) => boolean,
  description: string,
): Promise<void> {
  let last: [CoreState, number, RunType] | null = null;
  try {
    await browser.waitUntil(
      async () => {
        last = await invoke<[CoreState, number, RunType]>('get_core_status');
        return predicate(last[0], last[2]);
      },
      { timeout: 60_000, interval: 250, timeoutMsg: description },
    );
  } catch (error) {
    throw new Error(description + '. Last state: ' + JSON.stringify(last), {
      cause: error,
    });
  }
}

async function patchVerge(payload: Partial<VergeConfig>): Promise<void> {
  await invoke<null>('patch_verge_config', { payload });
}

async function runTunSmoke(): Promise<TunSmokeReport> {
  const result = await execFileAsync(
    'powershell.exe',
    [
      '-NoProfile',
      '-NonInteractive',
      '-ExecutionPolicy',
      'Bypass',
      '-File',
      tunSmokeScript,
      '-ServiceExe',
      serviceBinary,
      '-SkipTraffic',
    ],
    {
      cwd: repoRoot,
      timeout: 30_000,
      windowsHide: true,
      maxBuffer: 1024 * 1024,
    },
  );
  const report = JSON.parse(result.stdout) as TunSmokeReport;
  assert.equal(report.passed, true, JSON.stringify(report.checks, null, 2));
  assert.equal(
    report.checks.every((check) => check.passed),
    true,
    JSON.stringify(report.checks, null, 2),
  );
  return report;
}

async function collectCleanupError(
  errors: unknown[],
  cleanup: () => Promise<unknown>,
): Promise<void> {
  try {
    await cleanup();
  } catch (error) {
    errors.push(error);
  }
}

/**
 * T01/T02 contract: Windows system integration, not UI interaction coverage.
 *
 * Initial state: explicit opt-in dedicated VM/runner, elevated process, Service
 * not installed, isolated app Local core healthy.
 * Operation: install/start Service, hand off to Service Mode, enable TUN,
 * restart Service, then restore Local and uninstall the owned Service.
 * Independent result: app IPC plus scripts/windows-tun-smoke.ps1 checking the
 * real Service, runtime ownership, controller, adapter, and routes.
 * Regression caught: false success without Service handoff, TUN restoration,
 * or route convergence after a real Service restart.
 */
describe('Windows Service + TUN system lifecycle', () => {
  before(async () => {
    assert.equal(
      process.env.CHIMERA_E2E_SYSTEM_LIFECYCLE,
      '1',
      'Set CHIMERA_E2E_SYSTEM_LIFECYCLE=1 only on a dedicated runner/VM.',
    );
    assert.equal(process.platform, 'win32');
    assert.equal(
      await isElevated(),
      true,
      'Run the system lifecycle suite from an elevated Windows process.',
    );
    assert.equal(fs.existsSync(serviceBinary), true, serviceBinary);
    assert.equal(fs.existsSync(tunSmokeScript), true, tunSmokeScript);

    const initialService = await readServiceCliStatus();
    assert.equal(
      initialService.status,
      'not_installed',
      'Refusing to take ownership of an existing Chimera Service: ' +
        JSON.stringify(initialService),
    );
    await waitForCore(
      (state, runType) => state === 'Running' && runType !== 'service',
      'The isolated E2E app did not start with a healthy Local core',
    );
  });

  it('restores a Service-hosted TUN core after a real Service restart', async () => {
    let primaryError: unknown = null;
    const cleanupErrors: unknown[] = [];

    try {
      await invoke<null>('install_service');
      await waitForService(
        (status) => status.status === 'stopped' || status.status === 'running',
        'Installed Service did not become observable',
      );

      const installed = await readServiceCliStatus();
      assert.notEqual(installed.status, 'not_installed');
      if (installed.status !== 'running') {
        await invoke<null>('start_service');
      }
      await waitForService(
        (status) => status.status === 'running',
        'Service did not start',
      );

      await patchVerge({ enable_service_mode: true });
      await waitForCore(
        (state, runType) => state === 'Running' && runType === 'service',
        'Core did not hand off to Service Mode',
      );
      await waitForService(
        (status) =>
          status.status === 'running' &&
          status.phase === 'ready' &&
          status.runtime_owned,
        'Service host did not become ready for the E2E runtime',
      );

      await patchVerge({ enable_tun_mode: true });
      await waitForCore(
        (state, runType) => state === 'Running' && runType === 'service',
        'Service core did not remain running after TUN enable',
      );
      assert.equal(
        (await invoke<VergeConfig>('get_verge_config')).enable_tun_mode,
        true,
      );
      assert.equal(
        (await runTunSmoke()).checks.some(
          (check) => check.name === 'service_core_running' && check.passed,
        ),
        true,
      );

      await invoke<null>('restart_service');
      await waitForService(
        (status) =>
          status.status === 'running' &&
          status.phase === 'ready' &&
          status.runtime_owned,
        'Service did not become ready after restart',
      );
      await waitForCore(
        (state, runType) => state === 'Running' && runType === 'service',
        'Core did not recover on the Service host after restart',
      );
      assert.equal(
        (await runTunSmoke()).checks.some(
          (check) => check.name === 'service_core_running' && check.passed,
        ),
        true,
      );
    } catch (error) {
      primaryError = error;
    }

    await collectCleanupError(cleanupErrors, async () => {
      if ((await invoke<VergeConfig>('get_verge_config')).enable_tun_mode) {
        await patchVerge({ enable_tun_mode: false });
        await waitForCore(
          (state) => state === 'Running',
          'Core did not settle after TUN cleanup',
        );
      }
    });
    await collectCleanupError(cleanupErrors, async () => {
      if ((await invoke<VergeConfig>('get_verge_config')).enable_service_mode) {
        await patchVerge({ enable_service_mode: false });
        await waitForCore(
          (state, runType) => state === 'Running' && runType !== 'service',
          'Core did not hand back to Local during cleanup',
        );
      }
    });
    await collectCleanupError(cleanupErrors, async () => {
      if ((await readServiceCliStatus()).status === 'running') {
        await invoke<null>('stop_service');
        await waitForService(
          (status) => status.status !== 'running',
          'Service did not stop during cleanup',
        );
      }
    });
    await collectCleanupError(cleanupErrors, async () => {
      if ((await readServiceCliStatus()).status !== 'not_installed') {
        await invoke<null>('uninstall_service');
      }
      assert.equal((await readServiceCliStatus()).status, 'not_installed');
    });

    if (primaryError || cleanupErrors.length > 0) {
      throw new AggregateError(
        [primaryError, ...cleanupErrors].filter((error) => error !== null),
        'Windows Service/TUN lifecycle failed or cleanup was incomplete.',
      );
    }
  });
});
