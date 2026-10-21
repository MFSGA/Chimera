import assert from 'node:assert/strict';
import { createHash } from 'node:crypto';
import fs from 'node:fs';
import { tmpdir } from 'node:os';
import path from 'node:path';
import { fileURLToPath } from 'node:url';

interface VergeConfig {
  enable_auto_check_update?: boolean;
}

interface FrontendState {
  href: string;
  title: string;
  readyState: DocumentReadyState;
  rootChildCount: number;
  hasTauriInternals: boolean;
  visibilityState: DocumentVisibilityState;
}

interface FrontendObservation {
  phase: string;
  observedAt: string;
  state: FrontendState | null;
  error?: string;
  webDriverError?: boolean;
}

async function readFrontendState(): Promise<FrontendState> {
  return browser.execute(() => {
    const root = document.getElementById('root');
    return {
      href: window.location.href,
      title: document.title,
      readyState: document.readyState,
      rootChildCount: root?.childElementCount ?? 0,
      hasTauriInternals: '__TAURI_INTERNALS__' in window,
      visibilityState: document.visibilityState,
    };
  });
}

async function observeFrontend(
  phase: string,
  observations: FrontendObservation[],
): Promise<void> {
  try {
    observations.push({
      phase,
      observedAt: new Date().toISOString(),
      state: await readFrontendState(),
    });
  } catch (error) {
    observations.push({
      phase,
      observedAt: new Date().toISOString(),
      state: null,
      error: error instanceof Error ? error.message : String(error),
      webDriverError: true,
    });
    throw error;
  }
}

async function waitForApp(
  phase: string,
  observations: FrontendObservation[],
  timeout = 30_000,
) {
  let lastState: FrontendState | null = null;
  let observedAt: string | null = null;
  let readFailed = false;
  try {
    await browser.waitUntil(
      async () => {
        try {
          lastState = await readFrontendState();
        } catch (error) {
          readFailed = true;
          throw error;
        }
        observedAt = new Date().toISOString();
        return lastState.rootChildCount > 0;
      },
      {
        timeout,
        timeoutMsg: `The Chimera frontend did not render during ${phase}.`,
      },
    );
  } catch (cause) {
    observations.push({
      phase,
      observedAt: observedAt ?? new Date().toISOString(),
      state: lastState,
      error: cause instanceof Error ? cause.message : String(cause),
      webDriverError: readFailed,
    });
    throw new Error(
      `The Chimera frontend did not render during ${phase}. Last observed state: ${JSON.stringify(lastState)}.`,
      { cause },
    );
  }
  observations.push({
    phase,
    observedAt: observedAt ?? new Date().toISOString(),
    state: lastState,
  });
}

async function waitForDocumentComplete(
  phase: string,
  observations: FrontendObservation[],
): Promise<void> {
  let lastState: FrontendState | null = null;
  let observedAt: string | null = null;
  try {
    await browser.waitUntil(
      async () => {
        lastState = await readFrontendState();
        observedAt = new Date().toISOString();
        return lastState.readyState === 'complete';
      },
      {
        timeout: 30_000,
        interval: 500,
        timeoutMsg: `The Chimera document did not finish navigation during ${phase}.`,
      },
    );
  } catch (cause) {
    observations.push({
      phase,
      observedAt: observedAt ?? new Date().toISOString(),
      state: lastState,
      error: cause instanceof Error ? cause.message : String(cause),
    });
    throw new Error(
      `The Chimera document did not finish navigation during ${phase}. Last observed state: ${JSON.stringify(lastState)}.`,
      { cause },
    );
  }
  observations.push({
    phase,
    observedAt: observedAt ?? new Date().toISOString(),
    state: lastState,
  });
}

function errorDetails(error: unknown, seen = new Set<unknown>()): string {
  if (!(error instanceof Error)) return String(error);
  if (seen.has(error)) return '[circular error]';
  seen.add(error);
  const details = [error.stack ?? `${error.name}: ${error.message}`];
  if (error instanceof AggregateError) {
    details.push(
      ...Array.from(error.errors, (nested) => errorDetails(nested, seen)),
    );
  }
  if (error.cause !== undefined) {
    details.push(`Caused by: ${errorDetails(error.cause, seen)}`);
  }
  return details.join('\n');
}

async function appBinaryIdentity() {
  const defaultBinary = path.resolve(
    path.dirname(fileURLToPath(import.meta.url)),
    '../../backend/target/e2e/debug',
    process.platform === 'win32' ? 'chimera.exe' : 'chimera',
  );
  const binary = path.resolve(process.env.CHIMERA_E2E_BINARY ?? defaultBinary);
  try {
    const stat = fs.statSync(binary);
    const identity = {
      fileName: path.basename(binary),
      sizeBytes: stat.size,
      modifiedAt: stat.mtime.toISOString(),
    };
    if (!stat.isFile() || stat.size > 1024 * 1024 * 1024) {
      return { ...identity, sha256: null, hashUnavailable: 'unsupported file' };
    }

    const hash = createHash('sha256');
    const stream = fs.createReadStream(binary);
    let timeout: ReturnType<typeof setTimeout> | undefined;
    try {
      const sha256 = await Promise.race([
        (async () => {
          for await (const chunk of stream) hash.update(chunk);
          return hash.digest('hex');
        })(),
        new Promise<string>((_, reject) => {
          timeout = setTimeout(() => {
            stream.destroy();
            reject(new Error('E2E binary hash exceeded 5 seconds.'));
          }, 5_000);
        }),
      ]);
      return { ...identity, sha256 };
    } catch {
      return {
        ...identity,
        sha256: null,
        hashUnavailable: 'read failed or timed out',
      };
    } finally {
      if (timeout) clearTimeout(timeout);
    }
  } catch {
    return { fileName: path.basename(binary), sha256: null };
  }
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

async function readAutoUpdate(): Promise<boolean> {
  const config = await invoke<VergeConfig>('get_verge_config');
  return config.enable_auto_check_update ?? true;
}

async function patchAutoUpdate(value: boolean): Promise<void> {
  await invoke<null>('patch_verge_config', {
    payload: { enable_auto_check_update: value },
  });
}

function applicationConfigPath(): string {
  const runtime = process.env.CHIMERA_E2E_RUNTIME_DIR;
  assert.ok(
    runtime,
    'CHIMERA_E2E_RUNTIME_DIR must be set by the WDIO harness.',
  );
  return path.join(runtime, 'config', 'application.yaml');
}

function persistedAutoUpdate(file: string): boolean | null {
  if (!fs.existsSync(file)) return null;
  const match = /^enable_auto_check_update:\s*(true|false)\s*$/m.exec(
    fs.readFileSync(file, 'utf8'),
  );
  return match ? match[1] === 'true' : null;
}

async function waitForTypedPersistence(expected: boolean) {
  const file = applicationConfigPath();
  await browser.waitUntil(() => persistedAutoUpdate(file) === expected, {
    timeout: 15_000,
    timeoutMsg: `application.yaml did not persist enable_auto_check_update=${String(expected)}.`,
  });
}

type CaptureResult = 'captured' | 'failed' | 'timed-out';

async function captureBounded(
  label: string,
  action: () => Promise<unknown>,
  errors: string[],
): Promise<CaptureResult> {
  let timeout: ReturnType<typeof setTimeout> | undefined;
  let timedOut = false;
  try {
    await Promise.race([
      action(),
      new Promise<never>((_, reject) => {
        timeout = setTimeout(() => {
          timedOut = true;
          reject(new Error(`${label} capture exceeded 5 seconds.`));
        }, 5_000);
      }),
    ]);
    return 'captured';
  } catch (cause) {
    errors.push(`${label}: ${errorDetails(cause)}`);
    return timedOut ? 'timed-out' : 'failed';
  } finally {
    if (timeout) clearTimeout(timeout);
  }
}

async function captureFailureEvidence(
  error: unknown,
  kind: 'test' | 'cleanup',
  phase: string,
  attempt: number,
  expected: boolean | null,
  actual: boolean | null,
  observations: FrontendObservation[],
): Promise<string | null> {
  const runtime = process.env.CHIMERA_E2E_RUNTIME_DIR;
  const runId = (
    runtime ? path.basename(runtime) : `local-${process.pid}`
  ).replace(/[^a-zA-Z0-9_-]/g, '_');
  const directory =
    process.env.CHIMERA_E2E_ARTIFACT_DIR ??
    path.join(tmpdir(), 'chimera-e2e-artifacts');
  const prefix = path.join(
    directory,
    `application-typed-state-${runId}-persists-legacy-ipc-patches-attempt-${attempt}-${process.pid}-${Date.now()}`,
  );
  const captureErrors: string[] = [];

  try {
    fs.mkdirSync(directory, { recursive: true });
  } catch (cause) {
    process.stderr.write(
      `Could not create E2E evidence directory: ${errorDetails(cause)}\n`,
    );
    return null;
  }

  const lastObservation = observations.at(-1);
  const webDriverFailed = observations.some(
    (observation) => observation.webDriverError,
  );
  if (!browser.sessionId || webDriverFailed) {
    captureErrors.push('WebDriver session unavailable; skipped UI captures.');
  } else {
    const screenshot = await captureBounded(
      'screenshot',
      () => browser.saveScreenshot(`${prefix}.png`),
      captureErrors,
    );
    if (screenshot === 'captured') {
      await captureBounded(
        'page source',
        async () =>
          fs.writeFileSync(`${prefix}.html`, await browser.getPageSource()),
        captureErrors,
      );
    } else {
      // WDIO cannot cancel a timed-out command, so do not issue another capture.
      captureErrors.push(`Skipped page source after screenshot ${screenshot}.`);
    }
  }

  const capabilities = browser.capabilities;
  const binaryIdentity = await appBinaryIdentity();
  const diagnosticsPath = `${prefix}.json`;
  try {
    fs.writeFileSync(
      diagnosticsPath,
      JSON.stringify(
        {
          spec: 'application-typed-state.e2e.ts',
          test: 'persists legacy IPC patches through application.yaml and mirrors them back',
          attempt,
          phase,
          testError: kind === 'test' ? errorDetails(error) : null,
          cleanupError: kind === 'cleanup' ? errorDetails(error) : null,
          cleanup: 'pending',
          failedAt: new Date().toISOString(),
          platform: process.platform,
          nodeVersion: process.version,
          runtimeDirectory: runtime ? path.basename(runtime) : null,
          driver: {
            webdriverio: '9.32.0',
            tauriService: '@wdio/tauri-service 1.4.0',
            embeddedDriver:
              'tauri-plugin-wdio-webdriver 1.4.0 + e4bdb66 compatibility patch',
            browserName: capabilities.browserName ?? null,
            browserVersion: capabilities.browserVersion ?? null,
            platformName: capabilities.platformName ?? process.platform,
          },
          application: {
            buildId: process.env.GITHUB_SHA ?? null,
            ...binaryIdentity,
            core: 'not exercised by this spec',
          },
          expectedAutoUpdate: expected,
          lastObservedAutoUpdate: actual,
          persistedAutoUpdate: runtime
            ? persistedAutoUpdate(applicationConfigPath())
            : null,
          observations,
          captureErrors,
        },
        null,
        2,
      ),
      'utf8',
    );
    return diagnosticsPath;
  } catch (cause) {
    process.stderr.write(
      `Could not write E2E diagnostics: ${errorDetails(cause)}\n`,
    );
    return null;
  }
}

function recordCleanup(diagnosticsPath: string, error: unknown | null): void {
  try {
    const diagnostics = JSON.parse(
      fs.readFileSync(diagnosticsPath, 'utf8'),
    ) as {
      cleanup?: string;
      cleanupError?: string | null;
    };
    diagnostics.cleanup = error === null ? 'passed' : 'failed';
    diagnostics.cleanupError = error === null ? null : errorDetails(error);
    fs.writeFileSync(diagnosticsPath, JSON.stringify(diagnostics, null, 2));
  } catch (cause) {
    process.stderr.write(
      `Could not record E2E cleanup outcome: ${errorDetails(cause)}\n`,
    );
  }
}

// Contract: docs/testing/contracts/application-typed-state.md
describe('typed application state ownership', () => {
  it('persists legacy IPC patches through application.yaml and mirrors them back', async function (this: Mocha.Context) {
    this.timeout(180_000);
    const observations: FrontendObservation[] = [];
    const test = this.test as unknown as { currentRetry?: () => number };
    const attempt = (test.currentRetry?.() ?? 0) + 1;
    let phase = 'initial render';
    let original: boolean | undefined;
    let expected: boolean | null = null;
    let actual: boolean | null = null;
    let testFailed = false;
    let testError: unknown;
    let cleanupFailed = false;
    let cleanupError: unknown;
    let diagnosticsPath: string | null = null;

    try {
      await waitForApp(phase, observations);
      original = await readAutoUpdate();
      const changed = !original;
      expected = changed;

      phase = 'patching typed application state';
      await patchAutoUpdate(changed);
      await browser.waitUntil(
        async () => {
          actual = await readAutoUpdate();
          return actual === changed;
        },
        {
          timeout: 15_000,
          timeoutMsg:
            'Legacy get_verge_config did not observe the typed application patch.',
        },
      );
      await waitForTypedPersistence(changed);

      phase = 'before refresh';
      await observeFrontend(phase, observations);
      phase = 'refresh';
      try {
        await browser.refresh();
      } catch (error) {
        observations.push({
          phase,
          observedAt: new Date().toISOString(),
          state: null,
          error: error instanceof Error ? error.message : String(error),
          webDriverError: true,
        });
        throw error;
      }
      phase = 'refresh returned';
      await observeFrontend(phase, observations);
      phase = 'document load after refresh';
      await waitForDocumentComplete(phase, observations);
      phase = 'render after refresh';
      await waitForApp(phase, observations, 60_000);

      actual = await readAutoUpdate();
      assert.equal(actual, changed);
      assert.equal(persistedAutoUpdate(applicationConfigPath()), changed);
    } catch (error) {
      testFailed = true;
      testError = error;
      if (observations.at(-1)?.phase !== phase) {
        observations.push({
          phase,
          observedAt: new Date().toISOString(),
          state: null,
          error: error instanceof Error ? error.message : String(error),
        });
      }
      diagnosticsPath = await captureFailureEvidence(
        error,
        'test',
        phase,
        attempt,
        expected,
        actual,
        observations,
      );
    } finally {
      if (original !== undefined) {
        try {
          await patchAutoUpdate(original);
          await browser.waitUntil(
            async () => (await readAutoUpdate()) === original,
            {
              timeout: 15_000,
              timeoutMsg:
                'Application setting did not restore through the typed state owner.',
            },
          );
          await waitForTypedPersistence(original);
        } catch (error) {
          cleanupFailed = true;
          cleanupError = error;
          if (!testFailed) {
            diagnosticsPath = await captureFailureEvidence(
              error,
              'cleanup',
              'restore application setting',
              attempt,
              expected,
              actual,
              observations,
            );
          }
        }
      }
    }

    if (diagnosticsPath) {
      recordCleanup(diagnosticsPath, cleanupFailed ? cleanupError : null);
    }
    if (testFailed && cleanupFailed) {
      throw new AggregateError(
        [testError, cleanupError],
        'The typed application state test failed and cleanup also failed.',
        { cause: testError },
      );
    }
    if (testFailed) throw testError;
    if (cleanupFailed) throw cleanupError;
  });
});
