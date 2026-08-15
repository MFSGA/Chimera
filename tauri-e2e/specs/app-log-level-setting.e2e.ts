import assert from 'node:assert/strict';

const settingsPath = '/main/settings/nyanpasu';
const appLogLevelSelect = '#verge-app-log-level';
const levels = ['trace', 'debug', 'info', 'warn', 'error', 'silent'] as const;
type AppLogLevel = (typeof levels)[number];

async function waitForApp() {
  await browser.waitUntil(
    async () =>
      browser.execute(
        () => (document.getElementById('root')?.childElementCount ?? 0) > 0,
      ),
    { timeout: 30_000, timeoutMsg: 'The Chimera frontend did not render.' },
  );
}

async function openSettings() {
  const currentUrl = new URL(await browser.getUrl());
  currentUrl.pathname = settingsPath;
  currentUrl.search = '';
  currentUrl.hash = '';
  await browser.url(currentUrl.href);
  await waitForApp();

  const select = await $(appLogLevelSelect);
  await select.waitForDisplayed({ timeout: 15_000 });
  await select.waitForClickable({ timeout: 15_000 });
}

async function readAppLogLevel(): Promise<AppLogLevel> {
  return browser.execute(async () => {
    const tauri = (
      window as typeof window & {
        __TAURI_INTERNALS__: {
          invoke: (command: string) => Promise<{ app_log_level?: AppLogLevel }>;
        };
      }
    ).__TAURI_INTERNALS__;
    const config = await tauri.invoke('get_verge_config');
    return config.app_log_level ?? 'info';
  });
}

async function openLogLevelMenu() {
  const select = await $(appLogLevelSelect);
  await select.waitForClickable({ timeout: 15_000 });
  await select.click();
  await browser.waitUntil(
    async () =>
      browser.execute(
        () =>
          document.querySelector('[role="menuitemcheckbox"][data-value]') !==
          null,
      ),
    { timeout: 15_000, timeoutMsg: 'The app log-level menu did not open.' },
  );
}

async function readCheckedLevel(): Promise<AppLogLevel | null> {
  await openLogLevelMenu();
  return browser.execute(() => {
    const checked = document.querySelector<HTMLElement>(
      '[role="menuitemcheckbox"][data-value][data-state="checked"]',
    );
    return (checked?.dataset.value as AppLogLevel | undefined) ?? null;
  });
}

async function closeMenu() {
  await browser.keys('Escape');
}

async function setAppLogLevel(level: AppLogLevel) {
  if ((await readAppLogLevel()) === level) return;

  await openLogLevelMenu();
  const option = await $(`[role="menuitemcheckbox"][data-value="${level}"]`);
  await option.waitForClickable({ timeout: 15_000 });
  await option.click();

  await browser.waitUntil(async () => (await readAppLogLevel()) === level, {
    timeout: 15_000,
    timeoutMsg: `App log level did not become ${level}.`,
  });
}

describe('Chimera app log-level setting', () => {
  it('persists a selected level and restores the original value', async () => {
    await openSettings();

    const original = await readAppLogLevel();
    assert.ok(
      levels.includes(original),
      `Unexpected app log level: ${original}`,
    );
    const changed: AppLogLevel = original === 'info' ? 'warn' : 'info';

    try {
      await setAppLogLevel(changed);
      await browser.refresh();
      await openSettings();
      assert.equal(await readAppLogLevel(), changed);
      assert.equal(await readCheckedLevel(), changed);
      await closeMenu();
    } finally {
      await setAppLogLevel(original);
      await browser.refresh();
      await openSettings();
      assert.equal(await readAppLogLevel(), original);
      assert.equal(await readCheckedLevel(), original);
      await closeMenu();
    }
  });
});
