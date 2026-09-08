import assert from 'node:assert/strict';
import { readClashRuntimeConfig } from '../clash-runtime.js';

const settingsPath = '/main/settings/clash';
const logLevelTrigger = '[data-slot="log-level-selector-card"] button';
const logLevelValue =
  '[data-slot="log-level-selector-card"] [data-slot="settings-card-content-item-label-description"]';
const labels: Record<string, string> = {
  debug: 'Debug',
  info: 'Info',
  warning: 'Warn',
  error: 'Error',
  silent: 'Silent',
};

async function waitForApp() {
  await browser.waitUntil(
    async () =>
      browser.execute(
        () => (document.getElementById('root')?.childElementCount ?? 0) > 0,
      ),
    { timeout: 30_000, timeoutMsg: 'The Chimera frontend did not render.' },
  );
}

async function readRuntimeLogLevel(): Promise<string> {
  const config = await readClashRuntimeConfig<{ 'log-level': string }>();
  return config['log-level'];
}

async function openClashSettings() {
  const currentHref = await browser.getUrl();
  await browser.url(new URL(settingsPath, currentHref).href);
  await waitForApp();

  const trigger = await $(logLevelTrigger);
  await trigger.waitForDisplayed({ timeout: 15_000 });
  await trigger.waitForClickable({ timeout: 15_000 });
}

async function readSelectedLabel(): Promise<string | null> {
  return browser.execute((selector) => {
    const value = document.querySelector<HTMLElement>(selector);
    return value?.textContent?.trim() ?? null;
  }, logLevelValue);
}

async function setLogLevel(level: string) {
  assert.ok(level in labels, `Unsupported log level: ${level}`);

  if (
    (await readRuntimeLogLevel()) === level &&
    (await readSelectedLabel()) === labels[level]
  ) {
    return;
  }

  const trigger = await $(logLevelTrigger);
  await browser.execute((element) => element.focus(), trigger);
  await browser.keys('Enter');

  const option = await $(
    `//*[@role="menuitemcheckbox" and normalize-space()="${labels[level]}"]`,
  );
  await option.waitForDisplayed({ timeout: 15_000 });
  await option.waitForClickable({ timeout: 15_000 });
  await option.click();

  await browser.waitUntil(
    async () =>
      (await readRuntimeLogLevel()) === level &&
      (await readSelectedLabel()) === labels[level],
    {
      timeout: 15_000,
      timeoutMsg: `Log level did not become ${level} in the runtime and UI.`,
    },
  );
}

describe('Chimera log-level runtime setting', () => {
  it('persists a selected level and restores the original value', async () => {
    await waitForApp();
    await openClashSettings();

    const original = await readRuntimeLogLevel();
    assert.ok(original in labels, `Unexpected original log level: ${original}`);
    const changed = original === 'info' ? 'warning' : 'info';

    try {
      await setLogLevel(changed);
      await browser.refresh();
      await waitForApp();

      await browser.waitUntil(
        async () =>
          (await readRuntimeLogLevel()) === changed &&
          (await readSelectedLabel()) === labels[changed],
        {
          timeout: 15_000,
          timeoutMsg: 'Log level did not remain persisted after a page reload.',
        },
      );
    } finally {
      await setLogLevel(original);
      await browser.refresh();
      await waitForApp();
      assert.equal(await readRuntimeLogLevel(), original);
      assert.equal(await readSelectedLabel(), labels[original]);
    }
  });
});
