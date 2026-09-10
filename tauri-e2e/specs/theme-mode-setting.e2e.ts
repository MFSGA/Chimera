import assert from 'node:assert/strict';
import { focusElement } from './interaction.js';
import { openMainRoute } from './main-window.js';

const settingsPath = '/main/settings/user-interface';
const themeModeTrigger = '[data-slot="theme-mode-selector-trigger"]';
const themeModes = ['light', 'dark', 'system'] as const;
type ThemeMode = (typeof themeModes)[number];

async function openUserInterfaceSettings() {
  await openMainRoute(settingsPath);

  const trigger = await $(themeModeTrigger);
  await focusElement(trigger);
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
  const trigger = await $(themeModeTrigger);
  await focusElement(trigger);
  await browser.keys('Enter');
  await browser.waitUntil(
    async () =>
      browser.execute(
        () => document.querySelector('[role="menuitemcheckbox"]') !== null,
      ),
    { timeout: 15_000, timeoutMsg: 'The theme-mode menu did not open.' },
  );
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
  const option = options[themeModes.indexOf(mode)];
  await focusElement(option);
  await browser.keys('Enter');

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
      await browser.refresh();
      await openUserInterfaceSettings();
      assert.equal(await readThemeMode(), changed);
    } finally {
      await setThemeMode(original);
      await browser.refresh();
      await openUserInterfaceSettings();
      assert.equal(await readThemeMode(), original);
    }
  });
});
