import assert from 'node:assert/strict';
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
  // The Windows screenshot showed the menu closed after a WebDriver click.
  // If a pointer click did not open the trigger, try Radix's real keyboard
  // shortcut on the focused trigger, never a synthetic DOM click.
  markE2eStep('theme', 'click theme menu trigger');
  const trigger = await displayedElement(themeModeTrigger);
  await clickElement(trigger);
  if ((await trigger.getAttribute('aria-expanded')) !== 'true') {
    markE2eStep('theme', 'open trigger with keyboard');
    await browser.keys('ArrowDown');
  }
  markE2eStep('theme', 'wait for visible theme menu');
  try {
    await displayedElement(
      '[data-slot="theme-mode-selector-menu"] [role="menuitemcheckbox"]',
    );
  } catch (cause) {
    const expanded = await trigger
      .getAttribute('aria-expanded')
      .catch(() => null);
    throw new Error(
      `Theme mode menu was not visible; aria-expanded=${expanded}`,
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
