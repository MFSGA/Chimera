import assert from 'node:assert/strict';
import fs from 'node:fs';
import path from 'node:path';
import { openMainRoute } from './main-window.js';

const targetPath = '/main/settings/system';

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
      const systemLink = sidebar?.querySelector<HTMLElement>(
        'a[href="/main/settings/system"]',
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
        systemLink: rect(systemLink ?? null),
        systemActive: systemLink?.getAttribute('data-active'),
        systemCurrent: systemLink?.getAttribute('aria-current'),
      };
    });

    assert.ok(state.viewport.width >= 1200, JSON.stringify(state));
    assert.ok(state.viewport.height >= 600, JSON.stringify(state));
    assert.ok(
      state.container && state.sidebar && state.content,
      JSON.stringify(state),
    );
    assert.ok(state.navigation && state.systemLink, JSON.stringify(state));
    assert.equal(state.navigationGroups, 4, JSON.stringify(state));
    assert.equal(state.navigationItems, 7, JSON.stringify(state));
    assert.equal(state.systemActive, 'true', JSON.stringify(state));
    assert.equal(state.systemCurrent, 'page', JSON.stringify(state));
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

  it('selects another settings category through a visible link', async () => {
    const external = await $('a[href="/main/settings/web-ui"]');
    await external.waitForClickable({ timeout: 15_000 });
    await external.click();
    await browser.waitUntil(
      async () =>
        browser.execute(() => {
          const selected = document.querySelector<HTMLElement>(
            'a[href="/main/settings/web-ui"]',
          );
          return (
            location.pathname === '/main/settings/web-ui' &&
            selected?.getAttribute('data-active') === 'true' &&
            selected?.getAttribute('aria-current') === 'page'
          );
        }),
      {
        timeout: 15_000,
        timeoutMsg: 'Web UI settings navigation did not become active.',
      },
    );
    assert.equal(
      new URL(await browser.getUrl()).pathname,
      '/main/settings/web-ui',
    );
  });
});
