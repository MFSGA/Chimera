import assert from 'node:assert/strict';
import fs from 'node:fs';
import { tmpdir } from 'node:os';
import path from 'node:path';
import { openMainRoute } from './main-window.js';

const targetPath = '/main/dashboard';

type AppAppearance = {
  theme_mode?: string | null;
};

let originalThemeMode: string | null | undefined;

async function invoke<T>(command: string, args?: Record<string, unknown>) {
  return browser.execute(
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
    const originalWindowSize = await browser.getWindowSize();
    const storedLayouts = await invoke<string | null>('get_storage_item', {
      key: 'dashboard-widgets',
    });
    const readVisibleWidgets = () =>
      browser.execute(() => ({
        sparklineIds: Array.from(
          document.querySelectorAll<HTMLElement>(
            '[data-slot="widget-sparkline-card"]',
          ),
          (widget) => widget.dataset.widgetId,
        ).sort(),
        hasProxyShortcuts: Boolean(
          document.querySelector('[data-slot="proxy-shortcut-buttons"]'),
        ),
        hasCoreShortcuts: Boolean(
          document.querySelector('[data-slot="current-core-card"]'),
        ),
      }));

    try {
      const initialWidgets = await readVisibleWidgets();

      await browser.setWindowSize(900, originalWindowSize.height);
      await browser.waitUntil(
        async () => browser.execute(() => window.innerWidth < 1_000),
        {
          timeout: 5_000,
          timeoutMsg: 'Dashboard window did not enter the narrow layout.',
        },
      );

      await browser.setWindowSize(
        originalWindowSize.width,
        originalWindowSize.height,
      );
      await browser.waitUntil(
        async () => browser.execute(() => window.innerWidth >= 1_000),
        {
          timeout: 5_000,
          timeoutMsg: 'Dashboard window did not return to its wide layout.',
        },
      );
      await browser.waitUntil(
        async () =>
          JSON.stringify(await readVisibleWidgets()) ===
          JSON.stringify(initialWidgets),
        {
          timeout: 5_000,
          timeoutMsg: 'Dashboard widgets did not return to their wide layout.',
        },
      );

      assert.equal(
        await invoke<string | null>('get_storage_item', {
          key: 'dashboard-widgets',
        }),
        storedLayouts,
        'A responsive layout projection must not overwrite saved layouts.',
      );
    } finally {
      await browser.setWindowSize(
        originalWindowSize.width,
        originalWindowSize.height,
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
          `Could not create dashboard failure evidence: ${String(captureError)}\n`,
        );
        return;
      }

      await browser
        .saveScreenshot(`${artifactBase}.before-cleanup.png`)
        .catch((captureError) => captureErrors.push(String(captureError)));
      await browser
        .getPageSource()
        .then((source) =>
          fs.writeFileSync(`${artifactBase}.before-cleanup.html`, source),
        )
        .catch((captureError) => captureErrors.push(String(captureError)));
      try {
        fs.writeFileSync(
          `${artifactBase}.before-cleanup.error.txt`,
          error instanceof Error ? formatErrorDetails(error) : String(error),
        );
      } catch (captureError) {
        captureErrors.push(String(captureError));
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
      if (cleanupErrors.length === 1) throw cleanupErrors[0];
      if (cleanupErrors.length > 1) {
        throw new AggregateError(
          cleanupErrors,
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
            [operationError, cleanupError],
            `Widget storage operation and failure cleanup both failed.\nOperation error:\n${formatErrorDetails(operationError)}\nCleanup error:\n${formatErrorDetails(cleanupError)}`,
          );
        }
        throw cleanupError;
      }

      if (operationFailed) throw operationError;
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
          [testError, cleanupError],
          `Dashboard widget configuration test and cleanup both failed.\nTest error:\n${formatErrorDetails(testError)}\nCleanup error:\n${formatErrorDetails(cleanupError)}`,
        );
      }
      throw cleanupError;
    }
    if (testFailed) throw testError;
  });
});
