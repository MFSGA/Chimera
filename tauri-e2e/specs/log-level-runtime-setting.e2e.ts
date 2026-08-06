import assert from 'node:assert/strict';

const settingsPath = '/main/settings/clash';
const logLevelSelect = '#runtime-config-log-level';
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
  const info = await browser.execute(async () => {
    const tauri = (
      window as typeof window & {
        __TAURI_INTERNALS__: {
          invoke: (command: string) => Promise<{
            secret?: string;
            server: string;
          }>;
        };
      }
    ).__TAURI_INTERNALS__;
    return tauri.invoke('get_clash_info');
  });
  const response = await fetch(`http://${info.server}/configs`, {
    headers: info.secret
      ? { Authorization: `Bearer ${info.secret}` }
      : undefined,
  });
  if (!response.ok) {
    throw new Error(`Clash config query failed: ${response.status}`);
  }

  const config = (await response.json()) as { 'log-level': string };
  return config['log-level'];
}

async function openClashSettings() {
  const currentHref = await browser.getUrl();
  await browser.url(new URL(settingsPath, currentHref).href);
  await waitForApp();

  const select = await $(logLevelSelect);
  await select.waitForDisplayed({ timeout: 15_000 });
  await select.waitForClickable({ timeout: 15_000 });
}

async function readSelectedLabel(): Promise<string | null> {
  return browser.execute((selector) => {
    const select = document.querySelector<HTMLElement>(selector);
    return select?.textContent?.trim() ?? null;
  }, logLevelSelect);
}

async function setLogLevel(level: string) {
  assert.ok(level in labels, `Unsupported log level: ${level}`);

  if (
    (await readRuntimeLogLevel()) === level &&
    (await readSelectedLabel()) === labels[level]
  ) {
    return;
  }

  const opened = await browser.execute((selector) => {
    const select = document.querySelector<HTMLElement>(selector);
    if (!select) return false;
    select.dispatchEvent(
      new MouseEvent('mousedown', { bubbles: true, button: 0 }),
    );
    return true;
  }, logLevelSelect);
  assert.equal(opened, true, 'The log-level select was not found.');

  const option = await $(`[role="option"][data-value="${level}"]`);
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
