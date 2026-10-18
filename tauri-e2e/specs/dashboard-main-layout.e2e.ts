import assert from 'node:assert/strict';
import fs from 'node:fs';
import { tmpdir } from 'node:os';
import path from 'node:path';
import { openMainRoute } from './main-window.js';

const targetPath = '/main/dashboard';

describe('main dashboard reference layout', () => {
  before(async () => {
    await browser.setWindowSize(1240, 638);
    await browser.execute(() => {
      localStorage.setItem(btoa('paraglide-language-cache'), 'zh-cn');
    });
    await openMainRoute(targetPath);
    await browser.setWindowSize(1240, 638);
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

    const enterEditMode = async () => {
      if (await isEditMode()) return;
      const menuOpen = await browser.execute(() =>
        Array.from(
          document.querySelectorAll<HTMLElement>('[role="menuitem"]'),
        ).some((item) => item.innerText.trim() === '编辑组件'),
      );
      if (!menuOpen) {
        const dashboard = await $('[data-slot="dashboard-container"]');
        await dashboard.click({ button: 'right' });
      }
      const editItem = await $(
        '//*[@role="menuitem" and normalize-space(.)="编辑组件"]',
      );
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
        const control = document.querySelector(
          'button[aria-label^="速率单位:"]',
        );
        return Boolean(control && control.getClientRects().length > 0);
      });

    const openTrafficConfig = async () => {
      if (!(await isTrafficConfigOpen())) {
        const trigger = await $(
          '[data-slot="widget-config-trigger"][data-widget-id="traffic-down"]',
        );
        await trigger.waitForDisplayed({ timeout: 5_000 });
        await trigger.click();
      }
      const unitToggle = await $('button[aria-label^="速率单位:"]');
      await waitForObservedValue(
        'Traffic unit control visibility',
        () => unitToggle.isDisplayed(),
        Boolean,
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
          error instanceof Error
            ? (error.stack ?? error.message)
            : String(error),
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

      await attemptCleanup(() => restoreWidgetConfigInvoke());
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

    const failNextWidgetConfigOperation = async (
      failedCommand: string,
      persistUntilReleased = false,
    ) => {
      // Clear and replace the test-owned interceptor in one WebDriver command
      // so both steps target the same active page context.
      const injection = await browser.execute(
        (commandToFail, keepFailing) => {
          type TauriInternals = {
            invoke: (
              command: string,
              args?: Record<string, unknown>,
            ) => Promise<unknown>;
            widgetConfigOriginalInvoke?: TauriInternals['invoke'];
            widgetConfigInjectedInvoke?: TauriInternals['invoke'];
            widgetConfigStopFailure?: () => void;
            widgetConfigFailureCalls?: number;
          };
          const internals = (
            window as Window & { __TAURI_INTERNALS__?: TauriInternals }
          ).__TAURI_INTERNALS__;
          if (!internals) {
            throw new Error('Tauri page internals are unavailable.');
          }

          const hadStaleMarker = Boolean(
            internals.widgetConfigOriginalInvoke ||
            internals.widgetConfigInjectedInvoke ||
            internals.widgetConfigStopFailure,
          );
          const staleOriginalInvoke = internals.widgetConfigOriginalInvoke;
          const staleInjectedInvoke = internals.widgetConfigInjectedInvoke;
          const staleInvokeMatches =
            Boolean(staleInjectedInvoke) &&
            internals.invoke === staleInjectedInvoke;
          internals.widgetConfigStopFailure?.();
          if (staleOriginalInvoke && staleInvokeMatches) {
            internals.invoke = staleOriginalInvoke;
          }
          delete internals.widgetConfigOriginalInvoke;
          delete internals.widgetConfigInjectedInvoke;
          delete internals.widgetConfigStopFailure;
          delete internals.widgetConfigFailureCalls;

          const originalInvoke = internals.invoke;
          let failureActive = true;
          internals.widgetConfigOriginalInvoke = originalInvoke;
          internals.widgetConfigFailureCalls = 0;
          internals.widgetConfigStopFailure = () => {
            failureActive = false;
          };
          const injectedInvoke: TauriInternals['invoke'] = async (
            command,
            args,
          ) => {
            if (
              failureActive &&
              command === commandToFail &&
              args?.key === 'dashboard-widget-configs'
            ) {
              internals.widgetConfigFailureCalls =
                (internals.widgetConfigFailureCalls ?? 0) + 1;
              if (!keepFailing) failureActive = false;
              throw new Error(`Injected ${commandToFail} failure.`);
            }
            return originalInvoke.call(internals, command, args);
          };
          internals.widgetConfigInjectedInvoke = injectedInvoke;
          internals.invoke = injectedInvoke;
          if (internals.invoke !== injectedInvoke) {
            throw new Error(
              'The widget storage failure could not wrap Tauri invoke.',
            );
          }
          return {
            installed:
              internals.invoke === internals.widgetConfigInjectedInvoke &&
              internals.widgetConfigOriginalInvoke === originalInvoke,
            hadStaleMarker,
            staleInvokeMatches,
          };
        },
        failedCommand,
        persistUntilReleased,
      );
      if (injection.hadStaleMarker) {
        console.warn(
          `Cleared stale Dashboard test interceptor state (invoke matched previous wrapper: ${injection.staleInvokeMatches}).`,
        );
      }
      assert.equal(
        injection.installed,
        true,
        `The storage failure interceptor must be installed in the active WebView (stale marker cleared: ${injection.hadStaleMarker}; stale invoke matched: ${injection.staleInvokeMatches}).`,
      );
    };

    const navigateMainRouteWithoutReload = async (pathname: string) => {
      // This is setup for the storage contract, not a navigation assertion.
      // Keep the injected invoke wrapper in the same WebView document.
      await browser.execute((target) => {
        history.pushState({}, '', target);
        window.dispatchEvent(new PopStateEvent('popstate'));
      }, pathname);
      await openMainRoute(pathname);
    };

    const stopWidgetConfigFailure = async () =>
      browser.execute(() => {
        type TauriInternals = {
          widgetConfigStopFailure?: () => void;
        };
        const internals = (
          window as Window & { __TAURI_INTERNALS__?: TauriInternals }
        ).__TAURI_INTERNALS__;
        if (!internals?.widgetConfigStopFailure) {
          throw new Error('The widget storage failure is not installed.');
        }
        internals.widgetConfigStopFailure();
      });

    const restoreWidgetConfigInvoke = async () =>
      browser.execute(() => {
        type TauriInternals = {
          invoke: (
            command: string,
            args?: Record<string, unknown>,
          ) => Promise<unknown>;
          widgetConfigOriginalInvoke?: TauriInternals['invoke'];
          widgetConfigInjectedInvoke?: TauriInternals['invoke'];
          widgetConfigStopFailure?: () => void;
          widgetConfigFailureCalls?: number;
        };
        const internals = (
          window as Window & { __TAURI_INTERNALS__?: TauriInternals }
        ).__TAURI_INTERNALS__;
        const originalInvoke = internals?.widgetConfigOriginalInvoke;
        if (!internals || !originalInvoke) return;
        internals.widgetConfigStopFailure?.();
        if (
          internals.widgetConfigInjectedInvoke &&
          internals.invoke === internals.widgetConfigInjectedInvoke
        ) {
          internals.invoke = originalInvoke;
        }
        delete internals.widgetConfigOriginalInvoke;
        delete internals.widgetConfigInjectedInvoke;
        delete internals.widgetConfigStopFailure;
        delete internals.widgetConfigFailureCalls;
      });

    const withRestoredWidgetConfigInvoke = async (
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
        await restoreWidgetConfigInvoke();
      } catch (cleanupError) {
        if (operationFailed) {
          throw new AggregateError(
            [operationError, cleanupError],
            'Widget storage operation and invoke cleanup both failed.',
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

    const waitForUnitControl = async () => {
      const unitToggle = await $('button[aria-label^="速率单位:"]');
      await waitForObservedValue(
        'Traffic widget settings save',
        () => unitToggle.isEnabled(),
        Boolean,
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
      await withRestoredWidgetConfigInvoke(async () => {
        await failNextWidgetConfigOperation('get_storage_item', true);
        await navigateMainRouteWithoutReload(targetPath);
        const injectionState = await browser.execute(() => {
          const internals = (
            window as Window & {
              __TAURI_INTERNALS__?: {
                widgetConfigOriginalInvoke?: unknown;
                widgetConfigInjectedInvoke?: unknown;
                invoke?: unknown;
                widgetConfigFailureCalls?: number;
              };
            }
          ).__TAURI_INTERNALS__;
          return {
            installed: Boolean(
              internals?.widgetConfigOriginalInvoke &&
              internals.invoke === internals.widgetConfigInjectedInvoke,
            ),
            path: location.pathname,
          };
        });
        assert.deepEqual(
          injectionState,
          { installed: true, path: targetPath },
          'The storage failure interceptor must survive Dashboard route setup.',
        );
        await waitForObservedValue(
          'Widget storage read failure injection',
          () =>
            browser.execute(() => {
              const internals = (
                window as Window & {
                  __TAURI_INTERNALS__?: {
                    widgetConfigFailureCalls?: number;
                  };
                }
              ).__TAURI_INTERNALS__;
              return internals?.widgetConfigFailureCalls ?? 0;
            }),
          (count) => count > 0,
        );
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
        await stopWidgetConfigFailure();
        const saveButton = await $(
          '//*[@data-slot="dashboard-edit-header"]//button[.//span[normalize-space(.)="保存"]]',
        );
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
      await (
        await $(
          '[data-slot="widget-config-trigger"][data-widget-id="traffic-down"]',
        )
      ).click();
      const futureVersionUnitToggle = await $(
        'button[aria-label^="速率单位:"]',
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
        await $('[aria-label="恢复默认设置"]').isEnabled(),
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
        'button[aria-label^="速率单位:"]',
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
      await (await $('[aria-label="恢复默认设置"]')).click();
      await waitForRateUnit('B/s');
      await waitForStoredUnit('bytes');
      await waitForUnitControl();

      await withRestoredWidgetConfigInvoke(async () => {
        await failNextWidgetConfigOperation('set_storage_item');
        await $('button[aria-label="速率单位: B/s"]').click();
        const saveError = await $(
          '[data-slot="widget-config-save-status"][data-save-state="error"]',
        );
        await waitForObservedValue(
          'Widget config save error',
          () => saveError.getAttribute('data-save-state'),
          (state) => state === 'error',
        );
        assert.equal(
          await readStoredUnit(),
          'bytes',
          'a failed setting write must leave the backend value unchanged',
        );
      });

      await (await $('[data-slot="widget-config-save-status"] button')).click();
      await waitForRateUnit('bit/s');
      await waitForStoredUnit('bits');
      await waitForUnitControl();

      await browser.refresh();
      await openMainRoute(targetPath);
      await waitForRateUnit('bit/s');

      await enterEditMode();
      await openTrafficConfig();
      await (await $('[aria-label="恢复默认设置"]')).click();
      await waitForRateUnit('B/s');
      await waitForStoredUnit('bytes');
      await waitForUnitControl();
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
          'Dashboard widget configuration test and cleanup both failed.',
        );
      }
      throw cleanupError;
    }
    if (testFailed) throw testError;
  });
});
