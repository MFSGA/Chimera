import assert from 'node:assert/strict';
import { clickElement, displayedElement } from './interaction.js';
import { openMainRoute } from './main-window.js';

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
  // Open the menu using a real pointer action instead of forcing focus
  // through script; the Windows screenshot showed the menu open on timeout.
  await clickElement(await displayedElement(themeModeTrigger));
  await $('[role="menuitemcheckbox"]').waitForDisplayed({ timeout: 15_000 });
}

async function setThemeMode(mode: ThemeMode) {
  if ((await readThemeMode()) === mode) return;

  await openThemeModeMenu();
  const options = await $$('[role="menuitemcheckbox"]');
  const optionCount = await options.length;
  assert.equal(
    optionCount,
    themeModes.length,
    'Unexpected theme-mode options.',
  );
  // A user chooses the visible menu item with a pointer. Avoid injecting DOM
  // focus and a synthetic keyboard path that obscures which action hung.
  await clickElement(options[themeModes.indexOf(mode)]);

  await browser.waitUntil(async () => (await readThemeMode()) === mode, {
    timeout: 15_000,
    timeoutMsg: `Theme mode did not become ${mode}.`,
  });
}

describe('Chimera theme-mode preference', () => {
  it('persists a selected mode and restores the original value', async () => {
    await openUserInterfaceSettings();

    const original = await readThemeMode();
    assert.ok(
      themeModes.includes(original),
      `Unexpected theme mode: ${original}`,
    );
    const changed: ThemeMode = original === 'dark' ? 'light' : 'dark';

    try {
      await setThemeMode(changed);
      assert.equal(await readThemeMode(), changed);
      await openMainRoute('/main/dashboard');
      await openUserInterfaceSettings();
      assert.equal(await readThemeMode(), changed);
    } finally {
      await setThemeMode(original);
      assert.equal(await readThemeMode(), original);
      await openMainRoute('/main/dashboard');
      await openUserInterfaceSettings();
      assert.equal(await readThemeMode(), original);
    }
  });
});
