import assert from 'node:assert/strict';
import { execFileSync } from 'node:child_process';
import { createHash } from 'node:crypto';
import fs from 'node:fs';
import { tmpdir } from 'node:os';
import path from 'node:path';
import { fileURLToPath } from 'node:url';
import { isDeepStrictEqual } from 'node:util';
import {
  FRONTEND_ERROR_REDACTION_RULES,
  sanitizeFrontendErrorCapture,
  sanitizeFrontendErrorMessage,
  type FrontendErrorCapture,
} from '../privacy-safe-error.js';
import { openMainRoute } from './main-window.js';

const targetPath = '/main/dashboard';

type AppAppearance = {
  theme_mode?: string | null;
};

let originalThemeMode: string | null | undefined;

async function invoke<T>(command: string, args?: Record<string, unknown>) {
  return withBoundedWebDriverRequest(() =>
    browser.execute(
      async (name, parameters) => {
        const internals = (
          window as typeof window & {
            __TAURI_INTERNALS__: {
              invoke: <R>(
                command: string,
                args?: Record<string, unknown>,
              ) => Promise<R>;
            };
          }
        ).__TAURI_INTERNALS__;
        return internals.invoke<T>(name, parameters);
      },
      command,
      args,
    ),
  );
}

function formatErrorDetails(error: unknown, seen = new Set<unknown>()): string {
  if (!(error instanceof Error)) return String(error);
  if (seen.has(error)) return '[circular error]';
  seen.add(error);

  const details = [error.stack ?? `${error.name}: ${error.message}`];
  if (error instanceof AggregateError) {
    details.push(
      ...Array.from(error.errors, (nestedError) =>
        formatErrorDetails(nestedError, seen),
      ),
    );
  }
  if (error.cause !== undefined) {
    details.push(`Caused by: ${formatErrorDetails(error.cause, seen)}`);
  }
  return details.join('\n');
}

function formatSafeErrorDetails(error: unknown): string {
  return sanitizeFrontendErrorMessage(formatErrorDetails(error));
}

function toSafeError(error: unknown): Error {
  return new Error(formatSafeErrorDetails(error));
}

type MutableWebDriverOptions = {
  connectionRetryCount?: number;
  connectionRetryTimeout?: number;
};

type CaptureResult<T> =
  | { ok: true; value: T }
  | { ok: false; error: unknown; sessionUnavailable: boolean };

type ResizeFailureEvidence = {
  metadataPath: string;
  metadata: Record<string, unknown>;
  errors: string[];
};

async function installDashboardFrontendErrorCapture(): Promise<void> {
  await browser.execute((redactionRules) => {
    type CapturedError = { type: string; message: string; at: number };
    type CaptureState = {
      events: CapturedError[];
      cleanup: () => void;
    };
    type WindowWithCapture = typeof window & {
      __CHIMERA_DASHBOARD_E2E_ERRORS__?: CaptureState;
    };

    const target = window as WindowWithCapture;
    target.__CHIMERA_DASHBOARD_E2E_ERRORS__?.cleanup();
    const events: CapturedError[] = [];
    const sanitize = (message: string): string =>
      redactionRules
        .reduce(
          (sanitized, rule) =>
            sanitized.replace(
              new RegExp(rule.pattern, rule.flags),
              rule.replacement,
            ),
          message,
        )
        .slice(0, 2_000);
    const format = (value: unknown): string => {
      if (value instanceof Error) {
        return `${value.name}: ${value.message}\n${value.stack ?? ''}`;
      }
      if (typeof value === 'string') return value;
      if (
        value === null ||
        typeof value === 'number' ||
        typeof value === 'boolean'
      ) {
        return String(value);
      }
      return '[non-string diagnostic value omitted]';
    };
    const record = (type: string, value: unknown) => {
      if (events.length >= 25) return;
      events.push({ type, message: sanitize(format(value)), at: Date.now() });
    };
    const onError = (event: ErrorEvent) => {
      event.preventDefault();
      record('error', event.error ?? event.message);
    };
    const onUnhandledRejection = (event: PromiseRejectionEvent) => {
      event.preventDefault();
      record('unhandledrejection', event.reason);
    };
    const originalConsoleError = console.error;
    const wrappedConsoleError: Console['error'] = (...args) => {
      const sanitized = sanitize(args.map(format).join(' '));
      record('console.error', sanitized);
      originalConsoleError.call(console, sanitized);
    };

    window.addEventListener('error', onError);
    window.addEventListener('unhandledrejection', onUnhandledRejection);
    console.error = wrappedConsoleError;
    target.__CHIMERA_DASHBOARD_E2E_ERRORS__ = {
      events,
      cleanup: () => {
        window.removeEventListener('error', onError);
        window.removeEventListener('unhandledrejection', onUnhandledRejection);
        if (console.error === wrappedConsoleError) {
          console.error = originalConsoleError;
        }
      },
    };
  }, FRONTEND_ERROR_REDACTION_RULES);
}

async function removeDashboardFrontendErrorCapture(): Promise<void> {
  await browser.execute(() => {
    const target = window as typeof window & {
      __CHIMERA_DASHBOARD_E2E_ERRORS__?: { cleanup: () => void };
    };
    target.__CHIMERA_DASHBOARD_E2E_ERRORS__?.cleanup();
    delete target.__CHIMERA_DASHBOARD_E2E_ERRORS__;
  });
}

async function readDashboardFrontendErrorCapture(): Promise<FrontendErrorCapture> {
  return browser.execute(() => {
    const target = window as typeof window & {
      __CHIMERA_DASHBOARD_E2E_ERRORS__?: {
        events: Array<{ type: string; message: string; at: number }>;
      };
    };
    const capture = target.__CHIMERA_DASHBOARD_E2E_ERRORS__;
    return capture
      ? { status: 'captured' as const, events: [...capture.events] }
      : { status: 'not-installed' as const, events: [] };
  });
}

async function withBoundedWebDriverRequest<T>(
  operation: () => Promise<T>,
): Promise<T> {
  const options = browser.options as MutableWebDriverOptions;
  const previousTimeout = options.connectionRetryTimeout;
  const previousRetryCount = options.connectionRetryCount;
  options.connectionRetryTimeout = 5_000;
  options.connectionRetryCount = 0;
  try {
    return await operation();
  } finally {
    if (previousTimeout === undefined) {
      delete options.connectionRetryTimeout;
    } else {
      options.connectionRetryTimeout = previousTimeout;
    }
    if (previousRetryCount === undefined) {
      delete options.connectionRetryCount;
    } else {
      options.connectionRetryCount = previousRetryCount;
    }
  }
}

async function captureWebDriverValue<T>(
  label: string,
  operation: () => Promise<T>,
  errors: string[],
): Promise<CaptureResult<T>> {
  try {
    return {
      ok: true,
      value: await withBoundedWebDriverRequest(operation),
    };
  } catch (error) {
    errors.push(`${label}: ${formatSafeErrorDetails(error)}`);
    return {
      ok: false,
      error,
      sessionUnavailable:
        /invalid session id|no such session|session is not active/i.test(
          formatErrorDetails(error),
        ),
    };
  }
}

function summarizeDashboardStorage(value: string | null): unknown {
  if (value === null) return null;
  try {
    const layouts = JSON.parse(value) as Record<string, unknown>;
    return {
      present: true,
      byteLength: Buffer.byteLength(value),
      layouts: Object.entries(layouts).map(([breakpoint, widgets]) => ({
        breakpoint,
        widgets: Array.isArray(widgets)
          ? widgets.map((widget) => {
              const entry = widget as Record<string, unknown>;
              return {
                type: typeof entry.type === 'string' ? entry.type : null,
                x: typeof entry.x === 'number' ? entry.x : null,
                y: typeof entry.y === 'number' ? entry.y : null,
                w: typeof entry.w === 'number' ? entry.w : null,
                h: typeof entry.h === 'number' ? entry.h : null,
              };
            })
          : null,
      })),
    };
  } catch {
    return {
      present: true,
      byteLength: Buffer.byteLength(value),
      parseable: false,
    };
  }
}

function getBinarySha256(binaryPath: string): Promise<string> {
  return new Promise((resolve, reject) => {
    const hash = createHash('sha256');
    const binary = fs.createReadStream(binaryPath, {
      signal: AbortSignal.timeout(5_000),
    });
    binary.on('data', (chunk) => hash.update(chunk));
    binary.on('error', reject);
    binary.on('end', () => resolve(hash.digest('hex')));
  });
}

async function getApplicationBuildIdentity(): Promise<Record<string, unknown>> {
  const repositoryRoot = path.resolve(
    path.dirname(fileURLToPath(import.meta.url)),
    '../..',
  );
  const binaryName = process.platform === 'win32' ? 'chimera.exe' : 'chimera';
  const binaryPath =
    process.env.CHIMERA_E2E_BINARY ??
    path.resolve(repositoryRoot, 'backend/target/e2e/debug', binaryName);
  let sourceCommit = process.env.GITHUB_SHA ?? null;
  if (!sourceCommit) {
    try {
      sourceCommit = execFileSync('git', ['rev-parse', 'HEAD'], {
        cwd: repositoryRoot,
        encoding: 'utf8',
        timeout: 1_000,
      }).trim();
    } catch {
      sourceCommit = null;
    }
  }

  try {
    const binary = fs.statSync(binaryPath);
    let binarySha256: string | null = null;
    let binaryHashUnavailable: string | null = null;
    try {
      binarySha256 = await getBinarySha256(binaryPath);
    } catch (error) {
      binaryHashUnavailable = formatSafeErrorDetails(error);
    }
    return {
      sourceCommit,
      binary: path.basename(binaryPath),
      binarySha256,
      binaryHashUnavailable,
      binarySizeBytes: binary.size,
      binaryModifiedAt: binary.mtime.toISOString(),
    };
  } catch (error) {
    return {
      sourceCommit,
      binary: path.basename(binaryPath),
      binaryUnavailable: formatSafeErrorDetails(error),
    };
  }
}

async function captureResizeFailureEvidence(
  attempt: number,
  phase: string,
  error: unknown,
  expected: unknown,
  expectedStorage: string | null,
  frontendErrorSnapshot?: FrontendErrorCapture,
): Promise<ResizeFailureEvidence> {
  const runtime = process.env.CHIMERA_E2E_RUNTIME_DIR;
  const runId = (
    runtime ? path.basename(runtime) : `local-${process.pid}`
  ).replace(/[^a-zA-Z0-9_-]/g, '_');
  const directory =
    process.env.CHIMERA_E2E_ARTIFACT_DIR ??
    path.join(tmpdir(), 'chimera-e2e-artifacts');
  const artifactBase = path.join(
    directory,
    `${runId}-dashboard-main-layout-settles-responsive-layout-changes-without-persisting-a-resize-projection-attempt-${attempt}-${process.pid}-${Date.now()}`,
  );
  const metadataPath = `${artifactBase}.json`;
  const errors: string[] = [];
  let observedUi: unknown = null;
  let observedStorage: string | null = null;
  let frontendErrors: FrontendErrorCapture = frontendErrorSnapshot ?? {
    status: browser.sessionId ? 'not-installed' : 'session-unavailable',
    events: [],
  };
  try {
    fs.mkdirSync(directory, { recursive: true });
  } catch (captureError) {
    errors.push(
      `Could not create dashboard failure evidence: ${formatSafeErrorDetails(captureError)}`,
    );
  }

  const capabilities = browser.capabilities as {
    browserName?: string;
    browserVersion?: string;
    platformName?: string;
  };
  if (!browser.sessionId) {
    errors.push('WebDriver session unavailable; skipped UI evidence.');
  } else {
    let sessionUnavailable = false;
    if (!frontendErrorSnapshot) {
      const capturedFrontendErrors = await captureWebDriverValue(
        'frontend errors',
        readDashboardFrontendErrorCapture,
        errors,
      );
      if (capturedFrontendErrors.ok) {
        frontendErrors = sanitizeFrontendErrorCapture(
          capturedFrontendErrors.value,
        );
      }
    }
    const screenshot = await captureWebDriverValue(
      'screenshot',
      () => browser.saveScreenshot(`${artifactBase}.before-cleanup.png`),
      errors,
    );
    sessionUnavailable = !screenshot.ok && screenshot.sessionUnavailable;
    if (!sessionUnavailable) {
      const snapshot = await captureWebDriverValue(
        'window and widget state',
        () =>
          browser.execute(() => {
            const describeElement = (element: HTMLElement | null) => {
              if (!element) return null;
              const bounds = element.getBoundingClientRect();
              const style = getComputedStyle(element);
              return {
                slot: element.dataset.slot ?? null,
                widgetId: element.dataset.widgetId ?? null,
                bounds: {
                  x: bounds.x,
                  y: bounds.y,
                  width: bounds.width,
                  height: bounds.height,
                },
                display: style.display,
                visibility: style.visibility,
              };
            };
            const widgets = Array.from(
              document.querySelectorAll<HTMLElement>(
                '[data-slot="widget-sparkline-card"]',
              ),
              (widget) => ({
                ...describeElement(widget),
                widgetId: widget.dataset.widgetId ?? null,
              }),
            ).sort((left, right) =>
              String(left.widgetId).localeCompare(String(right.widgetId)),
            );
            return {
              route: window.location.pathname,
              viewport: {
                width: window.innerWidth,
                height: window.innerHeight,
              },
              grid: describeElement(
                document.querySelector<HTMLElement>(
                  '[data-slot="dnd-grid-container"]',
                ),
              ),
              widgets,
              proxyShortcuts: describeElement(
                document.querySelector<HTMLElement>(
                  '[data-slot="proxy-shortcut-buttons"]',
                ),
              ),
              coreShortcuts: describeElement(
                document.querySelector<HTMLElement>(
                  '[data-slot="current-core-card"]',
                ),
              ),
            };
          }),
        errors,
      );
      sessionUnavailable = !snapshot.ok && snapshot.sessionUnavailable;
      if (snapshot.ok) observedUi = snapshot.value;
    }
    if (!sessionUnavailable) {
      const pageSource = await captureWebDriverValue(
        'page source',
        () => browser.getPageSource(),
        errors,
      );
      sessionUnavailable = !pageSource.ok && pageSource.sessionUnavailable;
      if (pageSource.ok) {
        try {
          fs.writeFileSync(
            `${artifactBase}.before-cleanup.html`,
            pageSource.value,
          );
        } catch (captureError) {
          errors.push(
            `page source file: ${formatSafeErrorDetails(captureError)}`,
          );
        }
      }
    }
    if (!sessionUnavailable) {
      const storage = await captureWebDriverValue(
        'persisted Dashboard storage',
        () =>
          invoke<string | null>('get_storage_item', {
            key: 'dashboard-widgets',
          }),
        errors,
      );
      if (storage.ok) observedStorage = storage.value;
    } else {
      errors.push(
        'Skipped remaining UI evidence after the WebDriver session became unavailable.',
      );
    }
  }

  return {
    metadataPath,
    metadata: {
      spec: 'dashboard-main-layout.e2e.ts',
      test: 'settles responsive layout changes without persisting a resize projection',
      attempt,
      phase,
      expected,
      expectedStorage: summarizeDashboardStorage(expectedStorage),
      observed: {
        ui: observedUi,
        storage: summarizeDashboardStorage(observedStorage),
      },
      error: formatSafeErrorDetails(error),
      platform: process.platform,
      nodeVersion: process.version,
      runId,
      driver: {
        browserName: capabilities.browserName ?? null,
        browserVersion: capabilities.browserVersion ?? null,
        platformName: capabilities.platformName ?? process.platform,
      },
      application: await getApplicationBuildIdentity(),
      frontendErrors,
      kernel: 'not applicable to the Dashboard resize contract',
      cleanupResults: [],
      cleanupErrors: [],
      captureErrors: errors,
      capturedAt: new Date().toISOString(),
    },
    errors,
  };
}

function finalizeResizeFailureEvidence(
  evidence: ResizeFailureEvidence,
  cleanupResults: Array<{ phase: string; succeeded: boolean }>,
  cleanupErrors: string[],
): string[] {
  evidence.metadata.cleanupResults = cleanupResults;
  evidence.metadata.cleanupErrors = cleanupErrors;
  try {
    fs.writeFileSync(
      evidence.metadataPath,
      JSON.stringify(evidence.metadata, null, 2),
    );
  } catch (error) {
    evidence.errors.push(
      `Diagnostic metadata: ${formatSafeErrorDetails(error)}`,
    );
  }
  if (evidence.errors.length > 0) {
    process.stderr.write(
      `Dashboard failure evidence incomplete: ${evidence.errors.join('; ')}\n`,
    );
  }
  return evidence.errors;
}

async function refreshDashboardRouteForCleanup(): Promise<void> {
  await browser.execute((pathname) => {
    if (window.location.pathname !== pathname) {
      window.history.pushState({}, '', pathname);
      window.dispatchEvent(new PopStateEvent('popstate'));
    }
  }, targetPath);
  await browser.refresh();
  await browser.waitUntil(
    async () =>
      browser.execute(
        (pathname) =>
          window.location.pathname === pathname &&
          document.querySelector('[data-slot="app-root"]') !== null,
        targetPath,
      ),
    {
      timeout: 5_000,
      timeoutMsg: 'Dashboard route did not recover during test cleanup.',
    },
  );
}

let dashboardResizeAttempt = 0;

describe('main dashboard reference layout', () => {
  before(async () => {
    await browser.setWindowSize(1240, 638);
    const currentAppearance = await invoke<AppAppearance>('get_verge_config');
    originalThemeMode = currentAppearance.theme_mode ?? null;
    await invoke('patch_verge_config', {
      payload: { theme_mode: 'light' },
    });
    const configuredAppearance =
      await invoke<AppAppearance>('get_verge_config');
    assert.equal(
      configuredAppearance.theme_mode,
      'light',
      'Dashboard test light theme was not persisted.',
    );
    await openMainRoute(targetPath);
    await browser.waitUntil(
      async () =>
        browser.execute(() =>
          document.getElementById('root')?.classList.contains('light'),
        ),
      {
        timeout: 10_000,
        timeoutMsg: 'Dashboard test light theme did not apply to the app root.',
      },
    );
    await browser.setWindowSize(1240, 638);
  });

  after(async () => {
    if (originalThemeMode === undefined) return;
    await invoke('patch_verge_config', {
      payload: { theme_mode: originalThemeMode },
    });
    const restoredAppearance = await invoke<AppAppearance>('get_verge_config');
    assert.equal(restoredAppearance.theme_mode ?? null, originalThemeMode);
  });

  it('uses ref cards, real core artwork, and main settings links', async () => {
    const container = await $('[data-slot="dashboard-widget-container"]');
    await container.waitForDisplayed({ timeout: 15_000 });

    await browser.waitUntil(
      async () =>
        browser.execute(
          () =>
            document.querySelectorAll('[data-slot="widget-sparkline-card"]')
              .length >= 4,
        ),
      { timeout: 15_000, timeoutMsg: 'Dashboard widgets did not render.' },
    );

    const state = await browser.execute(() => {
      const dashboard = document.querySelector<HTMLElement>(
        '[data-slot="dashboard-container"]',
      );
      const container = document.querySelector<HTMLElement>(
        '[data-slot="dashboard-widget-container"]',
      );
      const sparklines = Array.from(
        document.querySelectorAll<HTMLElement>(
          '[data-slot="widget-sparkline-card"]',
        ),
      );
      const currentCore = document.querySelector<HTMLElement>(
        '[data-slot="current-core-card"]',
      );
      const coreIcon = document.querySelector<HTMLImageElement>(
        'img[data-slot="core-icon"]',
      );
      const systemLink = document.querySelector<HTMLAnchorElement>(
        'a[href="/main/settings/system"]',
      );
      const clashLink = document.querySelector<HTMLAnchorElement>(
        'a[href="/main/settings/clash"]',
      );
      const shortcutHeaders = Array.from(
        document.querySelectorAll<HTMLElement>('[data-slot="card-header"]'),
      );
      const firstTitle = document.querySelector<HTMLElement>(
        '[data-slot="widget-sparkline-card-title"]',
      );
      const firstValue = document.querySelector<HTMLElement>(
        '[data-slot="widget-sparkline-card-content"].text-2xl',
      );
      const firstCard = sparklines[0] ?? null;
      const style = (element: HTMLElement | null) => {
        if (!element) return null;
        const computed = getComputedStyle(element);
        return {
          position: computed.position,
          display: computed.display,
          flexDirection: computed.flexDirection,
          overflowX: computed.overflowX,
          overflowY: computed.overflowY,
          paddingTop: computed.paddingTop,
          paddingRight: computed.paddingRight,
          paddingBottom: computed.paddingBottom,
          paddingLeft: computed.paddingLeft,
          gap: computed.gap,
          fontSize: computed.fontSize,
          fontWeight: computed.fontWeight,
          lineHeight: computed.lineHeight,
          width: computed.width,
          height: computed.height,
        };
      };
      const rect = (element: HTMLElement | null) =>
        element
          ? {
              x: Math.round(element.getBoundingClientRect().x),
              y: Math.round(element.getBoundingClientRect().y),
              width: Math.round(element.getBoundingClientRect().width),
              height: Math.round(element.getBoundingClientRect().height),
            }
          : null;

      return {
        dashboard: { rect: rect(dashboard), style: style(dashboard) },
        container: { rect: rect(container), style: style(container) },
        firstCardStyle: style(firstCard),
        firstTitleStyle: style(firstTitle),
        firstValueStyle: style(firstValue),
        sparklineCount: sparklines.length,
        sparklineRects: sparklines.map(rect),
        currentCore: rect(currentCore),
        coreIcon: coreIcon
          ? {
              tagName: coreIcon.tagName,
              src: coreIcon.getAttribute('src') ?? '',
              rect: rect(coreIcon),
            }
          : null,
        hasSystemLink: Boolean(systemLink),
        hasClashLink: Boolean(clashLink),
        shortcutHeaderCount: shortcutHeaders.length,
        viewport: { width: window.innerWidth, height: window.innerHeight },
      };
    });

    assert.ok(state.viewport.width >= 1200, JSON.stringify(state, null, 2));
    assert.ok(state.viewport.height >= 600, JSON.stringify(state, null, 2));
    assert.ok(state.container.rect, JSON.stringify(state, null, 2));
    assert.equal(state.dashboard.style?.position, 'relative');
    assert.equal(state.dashboard.style?.display, 'flex');
    assert.equal(state.dashboard.style?.flexDirection, 'column');
    assert.equal(state.dashboard.style?.overflowX, 'hidden');
    assert.equal(state.dashboard.style?.overflowY, 'hidden');
    assert.deepEqual(
      [
        state.container.style?.paddingTop,
        state.container.style?.paddingRight,
        state.container.style?.paddingBottom,
        state.container.style?.paddingLeft,
      ],
      ['16px', '16px', '16px', '16px'],
      JSON.stringify(state, null, 2),
    );
    assert.equal(state.container.style?.display, 'flex');
    assert.equal(state.container.style?.flexDirection, 'column');
    assert.equal(state.firstCardStyle?.position, 'relative');
    assert.equal(state.firstTitleStyle?.display, 'flex');
    assert.equal(state.firstTitleStyle?.gap, '8px');
    assert.equal(state.firstValueStyle?.fontSize, '24px');
    assert.equal(state.firstValueStyle?.fontWeight, '700');
    assert.equal(state.firstValueStyle?.lineHeight, '32px');
    assert.ok(state.sparklineCount >= 4, JSON.stringify(state, null, 2));
    assert.equal(state.hasSystemLink, true, JSON.stringify(state, null, 2));
    assert.equal(state.hasClashLink, true, JSON.stringify(state, null, 2));
    assert.ok(state.shortcutHeaderCount >= 2, JSON.stringify(state, null, 2));
    assert.ok(state.currentCore, JSON.stringify(state, null, 2));
    assert.equal(
      state.coreIcon?.tagName,
      'IMG',
      JSON.stringify(state, null, 2),
    );
    assert.ok(
      (state.coreIcon?.rect?.width ?? 0) >= 40,
      JSON.stringify(state, null, 2),
    );
    assert.ok(
      (state.coreIcon?.src.length ?? 0) > 0,
      JSON.stringify(state, null, 2),
    );

    const evidencePath = process.env.CHIMERA_E2E_EVIDENCE_PATH;
    if (evidencePath) {
      fs.mkdirSync(path.dirname(evidencePath), { recursive: true });
      await browser.saveScreenshot(evidencePath);
    }
  });

  // Contract: docs/testing/contracts/dashboard-layout-resize.md
  it('settles responsive layout changes without persisting a resize projection', async () => {
    const attempt = ++dashboardResizeAttempt;
    const originalWindowSize = await withBoundedWebDriverRequest(() =>
      browser.getWindowSize(),
    );
    const storageKey = 'dashboard-widgets';
    const originalStorage = await invoke<string | null>('get_storage_item', {
      key: storageKey,
    });
    const testStorage = JSON.stringify({
      '4x5': [
        {
          id: 'traffic-down-narrow',
          type: 'traffic-down',
          x: 0,
          y: 0,
          w: 2,
          h: 2,
        },
      ],
      '8x6': [
        {
          id: 'traffic-down',
          type: 'traffic-down',
          x: 0,
          y: 0,
          w: 2,
          h: 2,
        },
        {
          id: 'traffic-up',
          type: 'traffic-up',
          x: 2,
          y: 0,
          w: 2,
          h: 2,
        },
        {
          id: 'memory-resize',
          type: 'memory',
          x: 4,
          y: 0,
          w: 2,
          h: 2,
        },
        {
          id: 'connections-resize',
          type: 'connections',
          x: 6,
          y: 0,
          w: 2,
          h: 2,
        },
        {
          id: 'proxy-shortcuts-resize',
          type: 'proxy-shortcuts',
          x: 0,
          y: 2,
          w: 3,
          h: 2,
        },
        {
          id: 'core-shortcuts-resize',
          type: 'core-shortcuts',
          x: 3,
          y: 2,
          w: 5,
          h: 2,
        },
      ],
    });
    const wideWidgets = {
      sparklineIds: [
        'connections-resize',
        'memory-resize',
        'traffic-down',
        'traffic-up',
      ],
      hasProxyShortcuts: true,
      hasCoreShortcuts: true,
    };
    const narrowWidgets = {
      sparklineIds: ['traffic-down-narrow'],
      hasProxyShortcuts: false,
      hasCoreShortcuts: false,
    };
    const readVisibleWidgets = () =>
      withBoundedWebDriverRequest(() =>
        browser.execute(() => ({
          sparklineIds: Array.from(
            document.querySelectorAll<HTMLElement>(
              '[data-slot="widget-sparkline-card"]',
            ),
            (widget) => widget.dataset.widgetId,
          )
            .filter((id): id is string => id !== undefined)
            .sort(),
          hasProxyShortcuts: Boolean(
            document.querySelector('[data-slot="proxy-shortcut-buttons"]'),
          ),
          hasCoreShortcuts: Boolean(
            document.querySelector('[data-slot="current-core-card"]'),
          ),
        })),
      );
    const waitForWidgets = async (
      description: string,
      expected: typeof wideWidgets,
    ) => {
      let lastObserved: typeof wideWidgets | null = null;
      try {
        await withBoundedWebDriverRequest(() =>
          browser.waitUntil(
            async () => {
              lastObserved = await readVisibleWidgets();
              return isDeepStrictEqual(lastObserved, expected);
            },
            {
              timeout: 10_000,
              timeoutMsg: `${description} did not settle to the expected widgets.`,
            },
          ),
        );
      } catch (error) {
        throw new Error(
          `${description}; last state: ${JSON.stringify(lastObserved ?? 'not observed')}.`,
          {
            cause: error,
          },
        );
      }
    };

    let primaryError: unknown;
    let phase = 'write deterministic dashboard layout fixture';
    let evidence: ResizeFailureEvidence | null = null;
    let frontendErrorSnapshot: FrontendErrorCapture | null = null;
    const evidenceErrors: string[] = [];
    const cleanupResults: Array<{ phase: string; succeeded: boolean }> = [];
    const cleanupErrors: Array<{ phase: string; error: unknown }> = [];
    try {
      await invoke('set_storage_item', {
        key: storageKey,
        value: testStorage,
      });
      phase = 'wait for the fixture in the local KV cache';
      await withBoundedWebDriverRequest(() =>
        browser.waitUntil(
          async () =>
            browser.execute(
              (key, expected) => {
                const cached = localStorage.getItem(
                  `nyanpasu-kv-:${btoa(key)}`,
                );
                return cached === expected;
              },
              storageKey,
              testStorage,
            ),
          {
            timeout: 5_000,
            timeoutMsg: 'Dashboard test fixture did not reach the local cache.',
          },
        ),
      );
      phase = 'refresh and open the Dashboard route';
      await withBoundedWebDriverRequest(() => browser.refresh());
      await withBoundedWebDriverRequest(() => openMainRoute(targetPath));
      phase = 'install frontend error capture';
      await withBoundedWebDriverRequest(installDashboardFrontendErrorCapture);
      phase = 'wait for the initial wide layout';
      await waitForWidgets('Wide Dashboard layout', wideWidgets);

      phase = 'resize the window to the narrow layout';
      await withBoundedWebDriverRequest(() =>
        browser.setWindowSize(400, originalWindowSize.height),
      );
      let narrowViewportWidth: number | null = null;
      try {
        await withBoundedWebDriverRequest(() =>
          browser.waitUntil(
            async () => {
              narrowViewportWidth = await browser.execute(
                () => window.innerWidth,
              );
              return narrowViewportWidth <= 400;
            },
            {
              timeout: 5_000,
              timeoutMsg:
                'Dashboard viewport did not reach the narrow grid width.',
            },
          ),
        );
      } catch (error) {
        throw new Error(
          `Dashboard viewport did not reach the narrow grid width; last viewport width=${narrowViewportWidth}.`,
          { cause: error },
        );
      }
      phase = 'wait for the narrow Dashboard layout';
      await waitForWidgets('Narrow Dashboard layout', narrowWidgets);

      phase = 'restore the original wide window size';
      await withBoundedWebDriverRequest(() =>
        browser.setWindowSize(
          originalWindowSize.width,
          originalWindowSize.height,
        ),
      );
      let restoredViewportWidth: number | null = null;
      try {
        await withBoundedWebDriverRequest(() =>
          browser.waitUntil(
            async () => {
              restoredViewportWidth = await browser.execute(
                () => window.innerWidth,
              );
              return restoredViewportWidth >= 1_000;
            },
            {
              timeout: 5_000,
              timeoutMsg: 'Dashboard window did not return to its wide layout.',
            },
          ),
        );
      } catch (error) {
        throw new Error(
          `Dashboard window did not return to its wide layout; last viewport width=${restoredViewportWidth}.`,
          { cause: error },
        );
      }
      phase = 'wait for the restored wide Dashboard layout';
      await waitForWidgets('Restored wide Dashboard layout', wideWidgets);

      phase = 'verify responsive projection did not change persisted storage';
      const actualStorage = await invoke<string | null>('get_storage_item', {
        key: storageKey,
      });
      assert.ok(
        actualStorage === testStorage,
        JSON.stringify({
          message:
            'A responsive layout projection must not overwrite saved layouts.',
          expected: summarizeDashboardStorage(testStorage),
          actual: summarizeDashboardStorage(actualStorage),
        }),
      );
    } catch (error) {
      primaryError = error;
      try {
        evidence = await captureResizeFailureEvidence(
          attempt,
          phase,
          error,
          { wideWidgets, narrowWidgets, storageUnchanged: true },
          testStorage,
        );
        frontendErrorSnapshot = evidence.metadata
          .frontendErrors as FrontendErrorCapture;
      } catch (captureError) {
        evidenceErrors.push(
          `Evidence capture threw: ${formatSafeErrorDetails(captureError)}`,
        );
      }
    } finally {
      if (!frontendErrorSnapshot) {
        const frontendErrors = await captureWebDriverValue(
          'frontend errors before cleanup',
          readDashboardFrontendErrorCapture,
          evidenceErrors,
        );
        frontendErrorSnapshot = frontendErrors.ok
          ? sanitizeFrontendErrorCapture(frontendErrors.value)
          : { status: 'session-unavailable', events: [] };
      }
      const runCleanupStep = async (
        cleanupPhase: string,
        operation: () => Promise<unknown>,
      ) => {
        try {
          await withBoundedWebDriverRequest(operation);
          cleanupResults.push({ phase: cleanupPhase, succeeded: true });
        } catch (error) {
          cleanupResults.push({ phase: cleanupPhase, succeeded: false });
          cleanupErrors.push({ phase: cleanupPhase, error });
          if (!evidence) {
            try {
              evidence = await captureResizeFailureEvidence(
                attempt,
                cleanupPhase,
                error,
                { route: targetPath, viewport: originalWindowSize },
                originalStorage,
                frontendErrorSnapshot ?? undefined,
              );
            } catch (captureError) {
              evidenceErrors.push(
                `Evidence capture threw: ${formatSafeErrorDetails(captureError)}`,
              );
            }
          }
        }
      };
      await runCleanupStep(
        'remove frontend error capture',
        removeDashboardFrontendErrorCapture,
      );
      await runCleanupStep('restore Dashboard storage', async () => {
        if (originalStorage === null) {
          await invoke('remove_storage_item', { key: storageKey });
        } else {
          await invoke('set_storage_item', {
            key: storageKey,
            value: originalStorage,
          });
        }
      });
      await runCleanupStep('restore the original window size', () =>
        browser.setWindowSize(
          originalWindowSize.width,
          originalWindowSize.height,
        ),
      );
      await runCleanupStep(
        'refresh and verify the restored Dashboard',
        async () => {
          await refreshDashboardRouteForCleanup();
          const restoredStorage = await invoke<string | null>(
            'get_storage_item',
            { key: storageKey },
          );
          assert.ok(
            restoredStorage === originalStorage,
            JSON.stringify({
              message: 'Dashboard test storage was not restored.',
              expected: summarizeDashboardStorage(originalStorage),
              actual: summarizeDashboardStorage(restoredStorage),
            }),
          );
        },
      );
    }

    if (evidence) {
      evidenceErrors.push(
        ...finalizeResizeFailureEvidence(
          evidence,
          cleanupResults,
          cleanupErrors.map(
            ({ phase: cleanupPhase, error }) =>
              `${cleanupPhase}: ${formatSafeErrorDetails(error)}`,
          ),
        ),
      );
    }
    if (primaryError || cleanupErrors.length > 0) {
      const failures = [
        ...(primaryError
          ? [`Primary failure:\n${formatSafeErrorDetails(primaryError)}`]
          : []),
        ...cleanupErrors.map(
          ({ phase: cleanupPhase, error }, index) =>
            `Cleanup failure ${index + 1} (${cleanupPhase}):\n${formatSafeErrorDetails(error)}`,
        ),
        ...evidenceErrors.map((error) => `Evidence capture issue:\n${error}`),
      ];
      throw new AggregateError(
        [
          ...(primaryError ? [toSafeError(primaryError)] : []),
          ...cleanupErrors.map(({ error }) => toSafeError(error)),
        ],
        [
          'Dashboard responsive resize failed or its cleanup was incomplete.',
          ...failures,
        ].join('\n\n'),
      );
    }
  });

  // Contract: docs/testing/contracts/dashboard-widget-config-retry.md
  it('recovers from failed widget storage and persists the retry', async () => {
    await openMainRoute(targetPath);
    async function waitForObservedValue<T>(
      description: string,
      observe: () => Promise<T>,
      matches: (value: T) => boolean,
    ) {
      let lastObserved: T | undefined;
      try {
        await browser.waitUntil(
          async () => {
            lastObserved = await observe();
            return matches(lastObserved);
          },
          {
            timeout: 10_000,
            timeoutMsg: `${description} did not reach its expected state.`,
          },
        );
      } catch (error) {
        throw new Error(
          `${description}; last observed: ${JSON.stringify(lastObserved)}.`,
          { cause: error },
        );
      }
    }

    const isEditMode = async () =>
      browser.execute(() =>
        Boolean(
          document.querySelector(
            '[data-slot="widget-config-trigger"][data-widget-id="traffic-down"]',
          ),
        ),
      );

    const editMenuItemSelector =
      '[data-slot="dashboard-context-menu-edit-widgets"]';
    const isEditMenuOpen = async () =>
      browser.execute(
        (selector) =>
          Boolean(
            document.querySelector(
              `[role="menu"][data-state="open"] ${selector}`,
            ),
          ),
        editMenuItemSelector,
      );

    const enterEditMode = async () => {
      if (await isEditMode()) return;
      if (!(await isEditMenuOpen())) {
        const dashboard = await $('[data-slot="dashboard-container"]');
        await dashboard.click({ button: 'right' });
      }
      const editItem = await $(editMenuItemSelector);
      await waitForObservedValue(
        'Dashboard edit menu',
        () => editItem.isDisplayed(),
        Boolean,
      );
      await editItem.click();
      await waitForObservedValue('Dashboard edit mode', isEditMode, Boolean);
    };

    const isTrafficConfigOpen = async () =>
      browser.execute(() => {
        const trigger = document.querySelector(
          '[data-slot="widget-config-trigger"][data-widget-id="traffic-down"]',
        );
        return trigger?.getAttribute('aria-expanded') === 'true';
      });

    const readTrafficConfigState = async () =>
      browser.execute(() => {
        const trigger = document.querySelector(
          '[data-slot="widget-config-trigger"][data-widget-id="traffic-down"]',
        );
        const unit = document.querySelector(
          '[data-slot="widget-config-choice"][data-config-field="speed-unit"]',
        );
        const reset = document.querySelector(
          '[data-slot="widget-config-reset"]',
        );
        const fields = document.querySelector(
          '[data-slot="widget-config-fields"]',
        );
        const isVisible = (element: Element | null) =>
          Boolean(
            element?.getClientRects().length &&
            getComputedStyle(element).visibility !== 'hidden',
          );

        return {
          pathname: location.pathname,
          editMode: Boolean(
            document.querySelector('[data-slot="widget-config-trigger"]'),
          ),
          triggerExpanded: trigger?.getAttribute('aria-expanded') ?? null,
          triggerState: trigger?.getAttribute('data-state') ?? null,
          fieldsMounted: fields !== null,
          unitVisible: isVisible(unit),
          unitDisabled: (unit as HTMLButtonElement | null)?.disabled ?? null,
          resetVisible: isVisible(reset),
          resetDisabled: (reset as HTMLButtonElement | null)?.disabled ?? null,
          saveState:
            document
              .querySelector('[data-slot="widget-config-save-status"]')
              ?.getAttribute('data-save-state') ?? null,
        };
      });

    const observeTrafficConfigState = async () => {
      const state = await readTrafficConfigState();
      const unitToggle = await $(
        '[data-slot="widget-config-choice"][data-config-field="speed-unit"]',
      );
      const resetButton = await $('[data-slot="widget-config-reset"]');
      return {
        ...state,
        unitDisplayed: await unitToggle.isDisplayed(),
        resetDisplayed: await resetButton.isDisplayed(),
      };
    };

    const ensureTrafficConfigOpen = async () => {
      if (!(await isTrafficConfigOpen())) {
        const trigger = await $(
          '[data-slot="widget-config-trigger"][data-widget-id="traffic-down"]',
        );
        await trigger.waitForDisplayed({ timeout: 5_000 });
        await trigger.click();
        await waitForObservedValue(
          'Traffic widget config Popover expansion',
          isTrafficConfigOpen,
          Boolean,
        );
      }
      await waitForObservedValue(
        'Traffic widget controls and Popover state',
        observeTrafficConfigState,
        (state) =>
          state.triggerExpanded === 'true' &&
          state.fieldsMounted &&
          state.unitDisplayed &&
          state.resetDisplayed,
      );
    };

    const openTrafficConfig = async () => {
      await ensureTrafficConfigOpen();
      const unitToggle = await $(
        '[data-slot="widget-config-choice"][data-config-field="speed-unit"]',
      );
      await waitForObservedValue(
        'Traffic widget settings loading',
        () => unitToggle.isEnabled(),
        Boolean,
      );
    };

    const readStoredValue = async (): Promise<string | null> =>
      browser.execute(async () => {
        const internals = (
          window as Window & {
            __TAURI_INTERNALS__?: {
              invoke: <T>(
                command: string,
                args?: Record<string, unknown>,
              ) => Promise<T>;
            };
          }
        ).__TAURI_INTERNALS__;
        if (!internals) {
          throw new Error('Tauri page internals are unavailable.');
        }
        return internals.invoke<string | null>('get_storage_item', {
          key: 'dashboard-widget-configs',
        });
      });

    const writeStoredValue = async (value: string | null) =>
      browser.execute(async (serializedValue) => {
        const internals = (
          window as Window & {
            __TAURI_INTERNALS__?: {
              invoke: (
                command: string,
                args?: Record<string, unknown>,
              ) => Promise<unknown>;
            };
          }
        ).__TAURI_INTERNALS__;
        if (!internals) {
          throw new Error('Tauri page internals are unavailable.');
        }
        if (serializedValue === null) {
          await internals.invoke('remove_storage_item', {
            key: 'dashboard-widget-configs',
          });
        } else {
          await internals.invoke('set_storage_item', {
            key: 'dashboard-widget-configs',
            value: serializedValue,
          });
        }
      }, value);

    const captureFailureBeforeCleanup = async (error: unknown) => {
      if (!browser.sessionId) return;
      const artifactDirectory =
        process.env.CHIMERA_E2E_ARTIFACT_DIR ??
        path.join(tmpdir(), 'chimera-e2e-artifacts');
      const runId = path.basename(
        process.env.CHIMERA_E2E_RUNTIME_DIR ?? `local-${process.pid}`,
      );
      const artifactBase = path.join(
        artifactDirectory,
        `dashboard-widget-config-${runId}-${process.pid}-${Date.now()}`,
      );
      const captureErrors: string[] = [];
      try {
        fs.mkdirSync(artifactDirectory, { recursive: true });
      } catch (captureError) {
        process.stderr.write(
          `Could not create dashboard failure evidence: ${formatSafeErrorDetails(captureError)}\n`,
        );
        return;
      }

      await browser
        .saveScreenshot(`${artifactBase}.before-cleanup.png`)
        .catch((captureError) =>
          captureErrors.push(formatSafeErrorDetails(captureError)),
        );
      await browser
        .getPageSource()
        .then((source) =>
          fs.writeFileSync(`${artifactBase}.before-cleanup.html`, source),
        )
        .catch((captureError) =>
          captureErrors.push(formatSafeErrorDetails(captureError)),
        );
      try {
        fs.writeFileSync(
          `${artifactBase}.before-cleanup.error.txt`,
          formatSafeErrorDetails(error),
        );
      } catch (captureError) {
        captureErrors.push(formatSafeErrorDetails(captureError));
      }
      if (captureErrors.length > 0) {
        process.stderr.write(
          `Dashboard failure evidence was incomplete: ${captureErrors.join('; ')}\n`,
        );
      }
    };

    const restoreOriginalWidgetConfig = async (
      originalValue: string | null,
    ) => {
      const cleanupErrors: unknown[] = [];
      const attemptCleanup = async (operation: () => Promise<unknown>) => {
        try {
          await operation();
        } catch (error) {
          cleanupErrors.push(error);
        }
      };

      await attemptCleanup(() => clearWidgetStorageFailure());
      let saveState: string | null = null;
      try {
        saveState = await browser.execute(
          () =>
            document
              .querySelector('[data-slot="widget-config-save-status"]')
              ?.getAttribute('data-save-state') ?? null,
        );
      } catch (error) {
        cleanupErrors.push(error);
      }
      if (saveState === 'saving') {
        await attemptCleanup(() =>
          waitForObservedValue(
            'Widget setting write completion before cleanup',
            () =>
              browser.execute(
                () =>
                  document
                    .querySelector('[data-slot="widget-config-save-status"]')
                    ?.getAttribute('data-save-state') ?? null,
              ),
            (state) => state !== 'saving',
          ),
        );
      }
      await attemptCleanup(() => writeStoredValue(originalValue));
      await attemptCleanup(async () =>
        assert.equal(
          await readStoredValue(),
          originalValue,
          'cleanup must restore the original dashboard widget configuration',
        ),
      );
      await attemptCleanup(() => browser.refresh());
      await attemptCleanup(() => openMainRoute(targetPath));
      await attemptCleanup(async () =>
        assert.equal(
          await readStoredValue(),
          originalValue,
          'the original widget configuration must remain after cleanup refresh',
        ),
      );
      if (cleanupErrors.length === 1) throw toSafeError(cleanupErrors[0]);
      if (cleanupErrors.length > 1) {
        throw new AggregateError(
          cleanupErrors.map(toSafeError),
          'Dashboard widget configuration cleanup had multiple failures.',
        );
      }
    };

    const readStoredUnit = async (): Promise<string | null> => {
      const raw = await readStoredValue();
      if (raw === null) return null;
      const storage = JSON.parse(raw) as {
        byInstance?: Record<string, { unit?: string }>;
      };
      return storage.byInstance?.['traffic-down']?.unit ?? null;
    };

    const waitForStoredUnit = async (expected: 'bytes' | 'bits') => {
      await waitForObservedValue(
        `Stored traffic unit ${expected}`,
        readStoredUnit,
        (unit) => unit === expected,
      );
    };

    type WidgetStorageFailureStatus = {
      operation: string | null;
      armed: boolean;
      triggered: boolean;
      calls: number;
    };

    const invokeE2eCommand = async <T>(
      command: string,
      args: Record<string, unknown> = {},
    ): Promise<T> =>
      browser.execute(
        async (name, parameters) => {
          const invoke = (
            window as Window & {
              __TAURI_INTERNALS__?: {
                invoke: <R>(
                  command: string,
                  args?: Record<string, unknown>,
                ) => Promise<R>;
              };
            }
          ).__TAURI_INTERNALS__?.invoke;
          if (!invoke) throw new Error('Tauri page internals are unavailable.');
          return invoke<T>(name, parameters);
        },
        command,
        args,
      );

    const armWidgetStorageFailure = async (
      operation: 'get_storage_item' | 'set_storage_item',
      persistUntilReleased = false,
    ) => {
      await invokeE2eCommand<void>('e2e_arm_widget_storage_failure', {
        operation,
        persistUntilReleased,
      });
    };

    const readWidgetStorageFailure = () =>
      invokeE2eCommand<WidgetStorageFailureStatus>(
        'e2e_widget_storage_failure_status',
      );

    const clearWidgetStorageFailure = () =>
      invokeE2eCommand<void>('e2e_clear_widget_storage_failure');

    const withClearedWidgetStorageFailure = async (
      operation: () => Promise<void>,
    ) => {
      let operationFailed = false;
      let operationError: unknown;
      try {
        await operation();
      } catch (error) {
        operationFailed = true;
        operationError = error;
      }

      try {
        await clearWidgetStorageFailure();
      } catch (cleanupError) {
        if (operationFailed) {
          throw new AggregateError(
            [toSafeError(operationError), toSafeError(cleanupError)],
            `Widget storage operation and failure cleanup both failed.\nOperation error:\n${formatSafeErrorDetails(operationError)}\nCleanup error:\n${formatSafeErrorDetails(cleanupError)}`,
          );
        }
        throw toSafeError(cleanupError);
      }

      if (operationFailed) throw toSafeError(operationError);
    };

    const waitForRateUnit = async (suffix: 'B/s' | 'bit/s') => {
      await waitForObservedValue(
        `Rendered traffic unit ${suffix}`,
        async () =>
          (
            await $(
              '[data-slot="widget-sparkline-card"][data-widget-id="traffic-down"] [data-slot="widget-sparkline-card-content"].text-2xl',
            )
          ).getText(),
        (value) => value.trim().endsWith(suffix),
      );
    };

    const originalStoredValue = await readStoredValue();
    let testFailed = false;
    let testError: unknown;
    try {
      await openMainRoute('/main/settings/system');
      await browser.refresh();
      await openMainRoute('/main/settings/system');
      await waitForObservedValue(
        'Dashboard provider unmounted before read failure injection',
        () =>
          browser.execute(() =>
            Boolean(
              document.querySelector('[data-slot="dashboard-container"]'),
            ),
          ),
        (mounted) => !mounted,
      );
      await withClearedWidgetStorageFailure(async () => {
        await armWidgetStorageFailure('get_storage_item', true);
        await openMainRoute(targetPath);
        await waitForObservedValue(
          'Widget storage read failure reached the real IPC command',
          async () => (await readWidgetStorageFailure()).calls,
          (calls) => calls > 0,
        );
        const readFailure = await readWidgetStorageFailure();
        assert.equal(readFailure.operation, 'get_storage_item');
        assert.equal(readFailure.armed, true);
        assert.equal(readFailure.triggered, true);
        await enterEditMode();
        await waitForObservedValue(
          'Widget config read error',
          () =>
            browser.execute(
              () =>
                document
                  .querySelector('[data-slot="widget-config-save-status"]')
                  ?.getAttribute('data-save-state') ?? null,
            ),
          (state) => state === 'load-error',
        );
        await clearWidgetStorageFailure();
        const saveButton = await $('[data-slot="dashboard-edit-save"]');
        await waitForObservedValue(
          'Dashboard edit exit after widget config read error',
          () => saveButton.isEnabled(),
          Boolean,
        );
        await saveButton.click();
        await waitForObservedValue(
          'Dashboard edit mode exit after widget config read error',
          isEditMode,
          (editing) => !editing,
        );
        await enterEditMode();
        const retryButton = await $(
          '[data-slot="dashboard-edit-header"] [data-slot="widget-config-save-status"] button',
        );
        await retryButton.click();
        await waitForObservedValue(
          'Widget config read retry',
          () =>
            browser.execute(
              () =>
                document
                  .querySelector(
                    '[data-slot="dashboard-edit-header"] [data-slot="widget-config-save-status"]',
                  )
                  ?.getAttribute('data-save-state') ?? null,
            ),
          (state) => state === null,
        );
      });

      const futureVersionValue = JSON.stringify({
        version: 2,
        byInstance: {
          'traffic-down': {
            type: 'traffic-down',
            unit: 'bits',
            futureOption: 'keep-me',
          },
        },
      });
      await writeStoredValue(futureVersionValue);
      await waitForObservedValue(
        'Unsupported widget config version from storage event',
        () =>
          browser.execute(
            () =>
              document
                .querySelector('[data-slot="widget-config-save-status"]')
                ?.getAttribute('data-save-state') ?? null,
          ),
        (state) => state === 'load-error',
      );
      await enterEditMode();
      await ensureTrafficConfigOpen();
      const futureVersionUnitToggle = await $(
        '[data-slot="widget-config-choice"][data-config-field="speed-unit"]',
      );
      await waitForObservedValue(
        'Widget setting control after unsupported config version',
        () => futureVersionUnitToggle.isDisplayed(),
        Boolean,
      );
      assert.equal(
        await futureVersionUnitToggle.isEnabled(),
        false,
        'settings must be disabled when persisted data uses an unknown version',
      );
      assert.equal(
        await $('[data-slot="widget-config-reset"]').isEnabled(),
        false,
        'reset must be disabled when persisted data uses an unknown version',
      );
      assert.equal(
        await readStoredValue(),
        futureVersionValue,
        'an unsupported version event must preserve the raw stored value',
      );

      await browser.refresh();
      await openMainRoute(targetPath);
      await enterEditMode();
      await waitForObservedValue(
        'Unsupported widget config version on initial read',
        () =>
          browser.execute(
            () =>
              document
                .querySelector('[data-slot="widget-config-save-status"]')
                ?.getAttribute('data-save-state') ?? null,
          ),
        (state) => state === 'load-error',
      );
      await (
        await $(
          '[data-slot="widget-config-trigger"][data-widget-id="traffic-down"]',
        )
      ).click();
      const futureVersionInitialReadToggle = await $(
        '[data-slot="widget-config-choice"][data-config-field="speed-unit"]',
      );
      await waitForObservedValue(
        'Widget setting control after unsupported config read',
        () => futureVersionInitialReadToggle.isDisplayed(),
        Boolean,
      );
      assert.equal(
        await futureVersionInitialReadToggle.isEnabled(),
        false,
        'settings must stay disabled when initial storage uses an unknown version',
      );
      assert.equal(
        await readStoredValue(),
        futureVersionValue,
        'an unsupported version read must preserve the raw stored value',
      );
      await writeStoredValue(JSON.stringify({ version: 1, byInstance: {} }));
      await browser.refresh();
      await openMainRoute(targetPath);
      await enterEditMode();
      await openTrafficConfig();

      await enterEditMode();
      await openTrafficConfig();
      await (await $('[data-slot="widget-config-reset"]')).click();
      await waitForRateUnit('B/s');
      await waitForStoredUnit('bytes');
      await openTrafficConfig();

      await withClearedWidgetStorageFailure(async () => {
        await armWidgetStorageFailure('set_storage_item');
        await $(
          '[data-slot="widget-config-choice"][data-config-field="speed-unit"]',
        ).click();
        const saveError = await $(
          '[data-slot="widget-config-save-status"][data-save-state="error"]',
        );
        await waitForObservedValue(
          'Widget config save error',
          () => saveError.getAttribute('data-save-state'),
          (state) => state === 'error',
        );
        assert.deepEqual(await readWidgetStorageFailure(), {
          operation: 'set_storage_item',
          armed: false,
          triggered: true,
          calls: 1,
        });
        assert.equal(
          await readStoredUnit(),
          'bytes',
          'a failed setting write must leave the backend value unchanged',
        );
      });

      await (await $('[data-slot="widget-config-save-status"] button')).click();
      await waitForRateUnit('bit/s');
      await waitForStoredUnit('bits');
      await openTrafficConfig();

      await browser.refresh();
      await openMainRoute(targetPath);
      await waitForRateUnit('bit/s');

      await enterEditMode();
      await openTrafficConfig();
      await (await $('[data-slot="widget-config-reset"]')).click();
      await waitForRateUnit('B/s');
      await waitForStoredUnit('bytes');
      await openTrafficConfig();
    } catch (error) {
      testFailed = true;
      testError = error;
      await captureFailureBeforeCleanup(error);
    }

    try {
      await restoreOriginalWidgetConfig(originalStoredValue);
    } catch (cleanupError) {
      if (testFailed) {
        throw new AggregateError(
          [toSafeError(testError), toSafeError(cleanupError)],
          `Dashboard widget configuration test and cleanup both failed.\nTest error:\n${formatSafeErrorDetails(testError)}\nCleanup error:\n${formatSafeErrorDetails(cleanupError)}`,
        );
      }
      throw toSafeError(cleanupError);
    }
    if (testFailed) throw toSafeError(testError);
  });
});
