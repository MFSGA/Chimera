import assert from 'node:assert/strict';

const settingsPath = '/main/settings/user-interface';
const themeModeTrigger = '[data-slot="theme-mode-selector"] button';
const themeModes = ['light', 'dark', 'system'] as const;
type ThemeMode = (typeof themeModes)[number];

async function waitForApp() {
  await browser.waitUntil(
    async () =>
      browser.execute(
        () => (document.getElementById('root')?.childElementCount ?? 0) > 0,
      ),
    { timeout: 30_000, timeoutMsg: 'The Chimera frontend did not render.' },
  );
}

async function openUserInterfaceSettings() {
  const currentHref = await browser.getUrl();
  await browser.url(new URL(settingsPath, currentHref).href);
  await waitForApp();

  const trigger = await $(themeModeTrigger);
  await trigger.waitForDisplayed({ timeout: 15_000 });
  await trigger.waitForClickable({ timeout: 15_000 });
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
  await trigger.waitForClickable({ timeout: 15_000 });
  const focused = await browser.execute((element) => {
    (element as HTMLElement).focus();
    return document.activeElement === element;
  }, trigger);
  assert.equal(focused, true, 'The theme-mode trigger was not focusable.');
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
  await option.waitForClickable({ timeout: 15_000 });
  await option.click();

  await browser.waitUntil(async () => (await readThemeMode()) === mode, {
    timeout: 15_000,
    timeoutMsg: `Theme mode did not become ${mode}.`,
  });
}

describe('Chimera theme-mode preference', () => {
  it('persists a selected mode and restores the original value', async () => {
    await waitForApp();
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
