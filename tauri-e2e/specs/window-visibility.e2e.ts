import assert from 'node:assert/strict';
import { displayedElement } from './interaction.js';
import { openMainRoute } from './main-window.js';

/*
 * Contract WIN-WEBVIEW-VISIBILITY (Windows desktop UI E2E; hermetic suite):
 * Start with the main window visible, click its real minimize control, and
 * observe document.visibilityState become hidden; restore it and require
 * visibility to return. The pre-fix WebView2 controller remained visible
 * after native minimization, so the hidden-state assertion catches regression.
 * Cleanup uses the Tauri window API only to restore the test-owned window.
 * This does not measure memory use or prove behavior on other platforms.
 */
async function restoreMainWindow() {
  await browser.switchToWindow('legacy');
  const errors = await browser.execute(async () => {
    const internals = (
      window as typeof window & {
        __TAURI_INTERNALS__: {
          invoke: (
            command: string,
            args?: Record<string, unknown>,
          ) => Promise<void>;
        };
      }
    ).__TAURI_INTERNALS__;
    const failures: string[] = [];

    for (const command of [
      'plugin:window|unminimize',
      'plugin:window|show',
      'plugin:window|set_focus',
    ]) {
      try {
        await internals.invoke(command, { label: 'main' });
      } catch (error) {
        failures.push(`${command}: ${String(error)}`);
      }
    }

    return failures;
  });

  await browser.switchToWindow('main');

  if (errors.length > 0) {
    throw new AggregateError(
      errors.map((message) => new Error(message)),
      'Failed to restore the main window after the visibility test.',
    );
  }

  await browser.waitUntil(
    async () => browser.execute(() => document.visibilityState === 'visible'),
    {
      timeout: 10_000,
      timeoutMsg: 'The main WebView did not become visible after restore.',
    },
  );
}

describe('Windows WebView visibility follows its native window', () => {
  it('hides when minimized and becomes visible after restore', async function () {
    if (process.platform !== 'win32') {
      this.skip();
    }

    await openMainRoute('/main/dashboard');
    const initialVisibility = await browser.execute(
      () => document.visibilityState,
    );
    assert.equal(initialVisibility, 'visible');

    let failure: unknown;
    try {
      const minimize = await displayedElement(
        '[data-slot="window-control-minimize-button"]',
      );
      await minimize.click();

      await browser.waitUntil(
        async () =>
          browser.execute(() => document.visibilityState === 'hidden'),
        {
          timeout: 10_000,
          timeoutMsg:
            'Minimizing the main window did not hide its WebView document.',
        },
      );
    } catch (error) {
      failure = error;
    }

    let cleanupFailure: unknown;
    try {
      await restoreMainWindow();
    } catch (error) {
      cleanupFailure = error;
    }

    if (failure && cleanupFailure) {
      throw new AggregateError(
        [failure, cleanupFailure],
        'Window visibility assertion and cleanup both failed.',
      );
    }
    if (failure) {
      throw failure;
    }
    if (cleanupFailure) {
      throw cleanupFailure;
    }
  });
});
