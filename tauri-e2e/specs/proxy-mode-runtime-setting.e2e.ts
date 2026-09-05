import assert from 'node:assert/strict';
import { readClashRuntimeConfig } from '../clash-runtime.js';

const mainPath = '/main/settings/clash';
const settingsMenuSelector = '[data-slot="header-settings-menu"]';
const proxySettingsTriggerSelector = '[data-slot="proxy-settings-trigger"]';

type ProxyMode = 'rule' | 'global' | 'direct' | 'script';

async function waitForFrontend() {
  await browser.waitUntil(
    async () =>
      browser.execute(
        () => (document.getElementById('root')?.childElementCount ?? 0) > 0,
      ),
    { timeout: 30_000, timeoutMsg: 'The Chimera frontend did not render.' },
  );
}

async function openDesktopShell() {
  await waitForFrontend();
  await browser.setWindowSize(1280, 900);

  const currentUrl = new URL(await browser.getUrl());
  currentUrl.pathname = mainPath;
  currentUrl.search = '';
  currentUrl.hash = '';
  await browser.url(currentUrl.href);
  await waitForFrontend();

  try {
    await browser.waitUntil(
      async () =>
        browser.execute(
          (selector) => document.querySelector(selector) !== null,
          settingsMenuSelector,
        ),
      {
        timeout: 15_000,
        timeoutMsg: 'The desktop Chimera shell did not render.',
      },
    );
  } catch (error) {
    const diagnostic = await browser.execute(
      (selector) => ({
        bodyText: document.body.innerText.slice(0, 300),
        hasAppRoot: document.querySelector('[data-slot="app-root"]') !== null,
        hasMenu: document.querySelector(selector) !== null,
        innerWidth: window.innerWidth,
        pathname: window.location.pathname,
        rootChildren: document.getElementById('root')?.childElementCount ?? 0,
      }),
      settingsMenuSelector,
    );
    throw new Error(
      `The desktop Chimera shell did not render: ${JSON.stringify(diagnostic)}`,
      { cause: error },
    );
  }
}

async function readRuntimeMode(): Promise<ProxyMode> {
  const config = await readClashRuntimeConfig<{ mode: ProxyMode }>();
  return config.mode.toLowerCase() as ProxyMode;
}

async function openProxyModeMenu() {
  const focused = await browser.execute((selector) => {
    const trigger = document.querySelector<HTMLElement>(selector);
    if (!trigger) return false;
    trigger.focus();
    return document.activeElement === trigger;
  }, settingsMenuSelector);
  assert.equal(focused, true, 'The settings menu trigger was not focusable.');
  await browser.keys('Enter');

  try {
    await browser.waitUntil(
      async () =>
        browser.execute(
          (selector) => document.querySelector(selector) !== null,
          proxySettingsTriggerSelector,
        ),
      {
        timeout: 15_000,
        timeoutMsg: 'The settings menu did not open.',
      },
    );
  } catch (error) {
    const diagnostic = await browser.execute((triggerSelector) => {
      const trigger = document.querySelector<HTMLElement>(triggerSelector);
      return {
        expanded: trigger?.getAttribute('aria-expanded') ?? null,
        menuItems: Array.from(
          document.querySelectorAll<HTMLElement>('[role="menuitem"]'),
        ).map((item) => item.innerText.trim()),
        menuCount: document.querySelectorAll('[role="menu"]').length,
        state: trigger?.dataset.state ?? null,
      };
    }, settingsMenuSelector);
    throw new Error(
      `The settings menu did not open: ${JSON.stringify(diagnostic)}`,
      { cause: error },
    );
  }

  const subTriggerFocused = await browser.execute((selector) => {
    const trigger = document.querySelector<HTMLElement>(selector);
    if (!trigger) return false;
    trigger.focus();
    return document.activeElement === trigger;
  }, proxySettingsTriggerSelector);
  assert.equal(
    subTriggerFocused,
    true,
    'The proxy settings submenu trigger was not focusable.',
  );
  await browser.keys('ArrowRight');

  await browser.waitUntil(
    async () =>
      browser.execute(
        () => document.querySelectorAll('[data-proxy-mode]').length > 0,
      ),
    {
      timeout: 15_000,
      timeoutMsg: 'The proxy-mode submenu did not open.',
    },
  );
}

async function readCheckedMode(): Promise<ProxyMode | null> {
  await openProxyModeMenu();
  return browser.execute(() => {
    const item = document.querySelector<HTMLElement>(
      '[data-proxy-mode][data-state="checked"]',
    );
    return (item?.dataset.proxyMode as ProxyMode | undefined) ?? null;
  });
}

async function closeMenus() {
  await browser.keys(['Escape', 'Escape']);
}

async function setMode(mode: ProxyMode) {
  if ((await readRuntimeMode()) === mode) return;

  await openProxyModeMenu();
  const option = await $(`[data-proxy-mode="${mode}"]`);
  await option.waitForClickable({ timeout: 15_000 });
  await option.click();

  await browser.waitUntil(async () => (await readRuntimeMode()) === mode, {
    timeout: 15_000,
    timeoutMsg: `The running core mode did not become ${mode}.`,
  });
}

describe('Chimera proxy mode runtime setting', () => {
  it('persists through the shared runtime transaction and restores the original mode', async () => {
    await openDesktopShell();

    const original = await readRuntimeMode();
    assert.ok(
      ['rule', 'global', 'direct', 'script'].includes(original),
      `Unexpected original proxy mode: ${original}`,
    );
    const changed: ProxyMode = original === 'global' ? 'rule' : 'global';

    try {
      await setMode(changed);
      await browser.refresh();
      await openDesktopShell();

      assert.equal(await readRuntimeMode(), changed);
      assert.equal(await readCheckedMode(), changed);
      await closeMenus();
    } finally {
      await setMode(original);
      await browser.refresh();
      await openDesktopShell();
      assert.equal(await readRuntimeMode(), original);
      assert.equal(await readCheckedMode(), original);
      await closeMenus();
    }
  });
});
