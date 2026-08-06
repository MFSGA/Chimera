import assert from 'node:assert/strict';

const settingsPath = '/main/settings/user-interface';
const themeModeSelect = '#user-interface-theme-mode';
const themeModes = ['dark', 'light', 'system'] as const;
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

  const select = await $(themeModeSelect);
  await select.waitForDisplayed({ timeout: 15_000 });
  await select.waitForClickable({ timeout: 15_000 });
}

async function readThemeMode(): Promise<ThemeMode> {
  const value = await browser.execute((selector) => {
    const select = document.querySelector<HTMLElement>(selector);
    const input = select?.closest('.MuiInputBase-root')?.querySelector('input');
    return input?.value ?? null;
  }, themeModeSelect);

  assert.ok(
    themeModes.includes(value as ThemeMode),
    `Unexpected theme mode value: ${String(value)}`,
  );
  return value as ThemeMode;
}

async function setThemeMode(mode: ThemeMode) {
  if ((await readThemeMode()) === mode) return;

  const opened = await browser.execute((selector) => {
    const select = document.querySelector<HTMLElement>(selector);
    if (!select) return false;
    select.dispatchEvent(
      new MouseEvent('mousedown', { bubbles: true, button: 0 }),
    );
    return true;
  }, themeModeSelect);
  assert.equal(opened, true, 'The theme-mode select was not found.');

  const option = await $(`[role="option"][data-value="${mode}"]`);
  await option.waitForDisplayed({ timeout: 15_000 });
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
    const changed: ThemeMode = original === 'dark' ? 'light' : 'dark';

    try {
      await setThemeMode(changed);
      await browser.refresh();
      await waitForApp();
      assert.equal(await readThemeMode(), changed);
    } finally {
      await setThemeMode(original);
      await browser.refresh();
      await waitForApp();
      assert.equal(await readThemeMode(), original);
    }
  });
});
