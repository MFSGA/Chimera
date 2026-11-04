import assert from 'node:assert/strict';
import fs from 'node:fs';
import path from 'node:path';
import { clickElement, displayedElement } from './interaction.js';
import { openMainRoute } from './main-window.js';
import { markE2eStep } from './step-diagnostics.js';

const settingsPath = '/main/settings/user-interface';
const themeModeTrigger = '[data-slot="theme-mode-selector-trigger"]';
const themeModes = ['light', 'dark', 'system'] as const;
type ThemeMode = (typeof themeModes)[number];

async function openUserInterfaceSettings() {
  await openMainRoute(settingsPath);
  await displayedElement(themeModeTrigger);
}

async function readThemeMode(): Promise<ThemeMode> {
  return browser.execute(async () => {
    const tauri = (
      window as typeof window & {
        __TAURI_INTERNALS__: {
          invoke: (command: string) => Promise<{ theme_mode?: ThemeMode }>;
        };
      }
    ).__TAURI_INTERNALS__;
    const config = await tauri.invoke('get_verge_config');
    return config.theme_mode ?? 'system';
  });
}

async function openThemeModeMenu() {
  // Open the menu through real pointer input, with Radix's keyboard
  // shortcut only if the trigger is still closed. Never synthesize a DOM click.
  markE2eStep('theme', 'click theme menu trigger');
  const trigger = await displayedElement(themeModeTrigger);
  await clickElement(trigger);
  if ((await trigger.getAttribute('aria-expanded')) !== 'true') {
    markE2eStep('theme', 'open trigger with keyboard');
    await browser.keys('ArrowDown');
  }
  markE2eStep('theme', 'wait for visible theme menu');
  try {
    // This selector intentionally matches all three mode options.
    // displayedElement() requires exactly ONE visible match and timed out
    // even though the failure-time screenshot showed three visible options.
    await $(
      '[data-slot="theme-mode-selector-menu"] [role="menuitemcheckbox"]',
    ).waitForDisplayed({ timeout: 15_000 });
  } catch (cause) {
    // Record the open menu *before* the test's finally block restores the
    // original preference. The generic afterTest screenshot is too late.
    const artifactDir = process.env.CHIMERA_E2E_ARTIFACT_DIR;
    let screenshot = 'unavailable';
    if (artifactDir) {
      const file = path.join(
        artifactDir,
        `theme-menu-before-cleanup-${Date.now()}.png`,
      );
      try {
        fs.mkdirSync(artifactDir, { recursive: true });
        await browser.saveScreenshot(file);
        screenshot = file;
      } catch (error) {
        screenshot = `capture failed: ${String(error)}`;
      }
    }
    const state = await browser
      .execute(() => ({
        pathname: location.pathname,
        expanded:
          document
            .querySelector('[data-slot="theme-mode-selector-trigger"]')
            ?.getAttribute('aria-expanded') ?? null,
        menuCount: document.querySelectorAll(
          '[data-slot="theme-mode-selector-menu"]',
        ).length,
        checkboxCount: document.querySelectorAll('[role="menuitemcheckbox"]')
          .length,
        visibleCheckboxCount: Array.from(
          document.querySelectorAll<HTMLElement>('[role="menuitemcheckbox"]'),
        ).filter(
          (item) =>
            item.getBoundingClientRect().width > 0 &&
            item.getBoundingClientRect().height > 0,
        ).length,
      }))
      .catch((error) => ({ diagnosticError: String(error) }));
    throw new Error(
      `Theme mode menu was not visible; state: ${JSON.stringify(state)}; screenshot: ${screenshot}`,
      { cause },
    );
  }
}

async function setThemeMode(mode: ThemeMode) {
  if ((await readThemeMode()) === mode) return;

  await openThemeModeMenu();
  markE2eStep('theme', 'select theme menu item');
  const options = await $$(
    '[data-slot="theme-mode-selector-menu"] [role="menuitemcheckbox"]',
  );
  const optionCount = await options.length;
  assert.equal(
    optionCount,
    themeModes.length,
    'Unexpected theme-mode options.',
  );
  // A user chooses the visible menu item with a pointer. Avoid injecting DOM
  // focus and a synthetic keyboard path that obscures which action hung.
  await clickElement(options[themeModes.indexOf(mode)]);

  markE2eStep('theme', 'verify selected theme persisted');
  await browser.waitUntil(async () => (await readThemeMode()) === mode, {
    timeout: 15_000,
    timeoutMsg: `Theme mode did not become ${mode}.`,
  });
}

describe('Chimera theme-mode preference', () => {
  it('persists a selected mode and restores the original value', async () => {
    markE2eStep('theme', 'open settings route');
    await openUserInterfaceSettings();

    markE2eStep('theme', 'read initial theme');
    const original = await readThemeMode();
    assert.ok(
      themeModes.includes(original),
      `Unexpected theme mode: ${original}`,
    );
    const changed: ThemeMode = original === 'dark' ? 'light' : 'dark';

    try {
      markE2eStep('theme', 'apply alternate theme');
      await setThemeMode(changed);
      assert.equal(await readThemeMode(), changed);
      markE2eStep('theme', 'navigate away to dashboard');
      await openMainRoute('/main/dashboard');
      await openUserInterfaceSettings();
      assert.equal(await readThemeMode(), changed);
    } finally {
      markE2eStep('theme', 'restore original theme');
      await setThemeMode(original);
      assert.equal(await readThemeMode(), original);
      await openMainRoute('/main/dashboard');
      await openUserInterfaceSettings();
      assert.equal(await readThemeMode(), original);
    }
  });
});
