import assert from 'node:assert/strict';
import fs from 'node:fs';
import path from 'node:path';
import { openMainRoute } from './main-window.js';

const targetPath = '/main/settings/system/proxy';

describe('main settings reference layout', () => {
  before(async () => {
    await browser.setWindowSize(1240, 638);
    await browser.execute(() => {
      localStorage.setItem(btoa('paraglide-language-cache'), 'zh-cn');
    });
    await openMainRoute(targetPath);
    await browser.setWindowSize(1240, 638);
  });

  it('renders grouped reference navigation and retains the full-height content layout', async () => {
    const content = await $('[data-slot="settings-content"]');
    await content.waitForDisplayed({ timeout: 15_000 });
    await browser.waitUntil(
      async () =>
        browser.execute(() => {
          const content = document.querySelector(
            '[data-slot="settings-content"]',
          );
          return content?.children.length === 1;
        }),
      {
        timeout: 15_000,
        timeoutMsg: 'The settings route transition did not settle.',
      },
    );

    const state = await browser.execute(() => {
      const container = document.querySelector<HTMLElement>(
        '[data-slot="settings-container"]',
      );
      const sidebar = document.querySelector<HTMLElement>(
        '[data-slot="settings-sidebar-scroll-area"]',
      );
      const content = document.querySelector<HTMLElement>(
        '[data-slot="settings-content"]',
      );
      const animatedOutlet = content?.firstElementChild as HTMLElement | null;
      const routeContent =
        animatedOutlet?.firstElementChild as HTMLElement | null;
      const nav = sidebar?.querySelector<HTMLElement>(
        '[data-slot="navigation-list"]',
      );
      const proxyLink = sidebar?.querySelector<HTMLElement>(
        'a[href="/main/settings/system/proxy"]',
      );
      const rect = (element: HTMLElement | null) => {
        if (!element) return null;
        const bounds = element.getBoundingClientRect();
        return {
          x: Math.round(bounds.x),
          y: Math.round(bounds.y),
          width: Math.round(bounds.width),
          height: Math.round(bounds.height),
        };
      };

      return {
        viewport: { width: innerWidth, height: innerHeight },
        container: rect(container),
        sidebar: rect(sidebar),
        content: rect(content),
        animatedOutlet: rect(animatedOutlet),
        routeContent: rect(routeContent),
        outletDisplay:
          animatedOutlet && getComputedStyle(animatedOutlet).display,
        outletDirection:
          animatedOutlet && getComputedStyle(animatedOutlet).flexDirection,
        outletGrow: animatedOutlet && getComputedStyle(animatedOutlet).flexGrow,
        navigation: rect(nav ?? null),
        navigationGroups:
          nav?.querySelectorAll('[data-slot="navigation-list-group"]').length ??
          0,
        navigationItems:
          nav?.querySelectorAll('[data-slot="navigation-list-item"]').length ??
          0,
        proxyLink: rect(proxyLink ?? null),
        proxyActive: proxyLink?.getAttribute('data-active'),
        proxyCurrent: proxyLink?.getAttribute('aria-current'),
      };
    });

    assert.ok(state.viewport.width >= 1200, JSON.stringify(state));
    assert.ok(state.viewport.height >= 600, JSON.stringify(state));
    assert.ok(
      state.container && state.sidebar && state.content,
      JSON.stringify(state),
    );
    assert.ok(state.navigation && state.proxyLink, JSON.stringify(state));
    assert.equal(state.navigationGroups, 4, JSON.stringify(state));
    assert.equal(state.navigationItems, 15, JSON.stringify(state));
    assert.equal(state.proxyActive, 'true', JSON.stringify(state));
    assert.equal(state.proxyCurrent, 'page', JSON.stringify(state));
    assert.equal(state.outletDisplay, 'flex', JSON.stringify(state));
    assert.equal(state.outletDirection, 'column', JSON.stringify(state));
    assert.equal(state.outletGrow, '1', JSON.stringify(state));
    assert.ok(
      (state.animatedOutlet?.height ?? 0) >= (state.container?.height ?? 0) - 2,
      JSON.stringify(state),
    );
    assert.ok(state.routeContent, JSON.stringify(state));

    const evidencePath = process.env.CHIMERA_E2E_EVIDENCE_PATH;
    if (evidencePath) {
      fs.mkdirSync(path.dirname(evidencePath), { recursive: true });
      await browser.saveScreenshot(evidencePath);
    }
  });

  it('navigates from proxy settings to service settings and back through visible links', async () => {
    const service = await $('a[href="/main/settings/system/service"]');
    await service.waitForClickable({ timeout: 15_000 });
    await service.click();

    const startup = await $('[data-slot="system-launch-container"]');
    await startup.waitForDisplayed({ timeout: 15_000 });
    await browser.waitUntil(
      async () =>
        browser.execute(() => {
          const selected = document.querySelector<HTMLElement>(
            'a[href="/main/settings/system/service"]',
          );
          return (
            location.pathname === '/main/settings/system/service' &&
            selected?.getAttribute('data-active') === 'true' &&
            selected?.getAttribute('aria-current') === 'page'
          );
        }),
      {
        timeout: 15_000,
        timeoutMsg: 'Service settings navigation did not become active.',
      },
    );

    const proxy = await $('a[href="/main/settings/system/proxy"]');
    await proxy.waitForClickable({ timeout: 15_000 });
    await proxy.click();

    const mode = await $('[data-slot="proxy-mode-container"]');
    await mode.waitForDisplayed({ timeout: 15_000 });
    await browser.waitUntil(
      async () =>
        browser.execute(() => {
          const selected = document.querySelector<HTMLElement>(
            'a[href="/main/settings/system/proxy"]',
          );
          return (
            location.pathname === '/main/settings/system/proxy' &&
            selected?.getAttribute('data-active') === 'true' &&
            selected?.getAttribute('aria-current') === 'page'
          );
        }),
      {
        timeout: 15_000,
        timeoutMsg: 'Proxy settings navigation did not become active.',
      },
    );
  });

  it('opens the reference control-channel settings through the sidebar', async () => {
    const control = await $('a[href="/main/settings/clash/control"]');
    await control.waitForClickable({ timeout: 15_000 });
    await control.click();

    await browser.waitUntil(
      async () =>
        browser.execute(() => {
          const selected = document.querySelector<HTMLElement>(
            'a[href="/main/settings/clash/control"]',
          );
          return (
            location.pathname === '/main/settings/clash/control' &&
            selected?.getAttribute('data-active') === 'true' &&
            selected?.getAttribute('aria-current') === 'page' &&
            document.querySelector(
              '[data-slot="web-ui-settings-container"]',
            ) !== null
          );
        }),
      {
        timeout: 15_000,
        timeoutMsg: 'Control-channel settings did not become active.',
      },
    );

    assert.equal(
      new URL(await browser.getUrl()).pathname,
      '/main/settings/clash/control',
    );
  });

  it('opens network settings and displays the existing port controls', async () => {
    const network = await $('a[href="/main/settings/clash/network"]');
    await network.waitForClickable({ timeout: 15_000 });
    await network.click();

    const portSettings = await $('[data-slot="port-settings-container"]');
    await portSettings.waitForDisplayed({ timeout: 15_000 });

    await browser.waitUntil(
      async () =>
        browser.execute(() => {
          const selected = document.querySelector<HTMLElement>(
            'a[href="/main/settings/clash/network"]',
          );
          return (
            location.pathname === '/main/settings/clash/network' &&
            selected?.getAttribute('data-active') === 'true' &&
            selected?.getAttribute('aria-current') === 'page'
          );
        }),
      {
        timeout: 15_000,
        timeoutMsg: 'Network settings did not become active.',
      },
    );

    assert.equal(
      new URL(await browser.getUrl()).pathname,
      '/main/settings/clash/network',
    );
  });

  it('opens the tray and window settings from the new reference navigation', async () => {
    const tray = await $('a[href="/main/settings/chimera/tray"]');
    await tray.waitForClickable({ timeout: 15_000 });
    await tray.click();

    const traySettings = await $('[data-slot="tray-settings-container"]');
    const windowSettings = await $('[data-slot="window-settings-container"]');
    await traySettings.waitForDisplayed({ timeout: 15_000 });
    await windowSettings.waitForDisplayed({ timeout: 15_000 });

    await browser.waitUntil(
      async () =>
        browser.execute(() => {
          const selected = document.querySelector<HTMLElement>(
            'a[href="/main/settings/chimera/tray"]',
          );
          return (
            location.pathname === '/main/settings/chimera/tray' &&
            selected?.getAttribute('data-active') === 'true' &&
            selected?.getAttribute('aria-current') === 'page'
          );
        }),
      {
        timeout: 15_000,
        timeoutMsg: 'Tray settings route did not become active.',
      },
    );
    assert.equal(
      new URL(await browser.getUrl()).pathname,
      '/main/settings/chimera/tray',
    );
  });

  it('opens public IP settings while retaining the original controls', async () => {
    const publicIp = await $('a[href="/main/settings/chimera/public-ip"]');
    await publicIp.waitForClickable({ timeout: 15_000 });
    await publicIp.click();

    const settings = await $('[data-slot="public-ip-settings"]');
    await settings.waitForDisplayed({ timeout: 15_000 });

    await browser.waitUntil(
      async () =>
        browser.execute(() => {
          const selected = document.querySelector<HTMLElement>(
            'a[href="/main/settings/chimera/public-ip"]',
          );
          return (
            location.pathname === '/main/settings/chimera/public-ip' &&
            selected?.getAttribute('data-active') === 'true' &&
            selected?.getAttribute('aria-current') === 'page'
          );
        }),
      {
        timeout: 15_000,
        timeoutMsg: 'Public IP settings route did not become active.',
      },
    );
    assert.equal(
      new URL(await browser.getUrl()).pathname,
      '/main/settings/chimera/public-ip',
    );
  });
});
