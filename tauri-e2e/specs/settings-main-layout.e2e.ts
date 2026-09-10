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

  it('keeps the ref sidebar and full-height flex content chain', async () => {
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
      const systemLink = sidebar?.querySelector<HTMLElement>(
        'a[href="/main/settings/system"]',
      );
      const systemLabel = systemLink?.querySelector<HTMLElement>(
        '.text-sm.font-medium',
      );
      const systemDescription =
        systemLink?.querySelector<HTMLElement>('[class*="text-xs"]');
      const externalCoreIcon = sidebar?.querySelector<HTMLImageElement>(
        'a[href="/main/settings/web-ui"] img',
      );
      const zincProbe = document.createElement('div');
      zincProbe.className = 'text-zinc-500';
      document.body.append(zincProbe);
      const zinc500Color = getComputedStyle(zincProbe).color;
      zincProbe.remove();
      const rect = (element: HTMLElement | null) =>
        element
          ? {
              x: Math.round(element.getBoundingClientRect().x),
              y: Math.round(element.getBoundingClientRect().y),
              width: Math.round(element.getBoundingClientRect().width),
              height: Math.round(element.getBoundingClientRect().height),
            }
          : null;
      const style = (element: HTMLElement | null) =>
        element
          ? {
              display: getComputedStyle(element).display,
              flexDirection: getComputedStyle(element).flexDirection,
              flexGrow: getComputedStyle(element).flexGrow,
            }
          : null;

      return {
        viewport: { width: innerWidth, height: innerHeight },
        container: rect(container),
        sidebar: rect(sidebar),
        content: rect(content),
        animatedOutlet: rect(animatedOutlet),
        animatedOutletStyle: style(animatedOutlet),
        routeContent: rect(routeContent),
        systemLink: rect(systemLink ?? null),
        externalCoreIcon: rect(externalCoreIcon ?? null),
        systemLabelStyle: systemLabel
          ? {
              textOverflow: getComputedStyle(systemLabel).textOverflow,
              whiteSpace: getComputedStyle(systemLabel).whiteSpace,
            }
          : null,
        systemDescriptionColor: systemDescription
          ? getComputedStyle(systemDescription).color
          : null,
        zinc500Color,
      };
    });

    assert.ok(state.viewport.width >= 1200, JSON.stringify(state, null, 2));
    assert.ok(state.viewport.height >= 600, JSON.stringify(state, null, 2));
    assert.ok(state.container, JSON.stringify(state, null, 2));
    assert.ok(state.sidebar, JSON.stringify(state, null, 2));
    assert.ok(state.content, JSON.stringify(state, null, 2));
    assert.ok(state.animatedOutlet, JSON.stringify(state, null, 2));
    assert.equal(
      state.animatedOutletStyle?.display,
      'flex',
      JSON.stringify(state, null, 2),
    );
    assert.equal(
      state.animatedOutletStyle?.flexDirection,
      'column',
      JSON.stringify(state, null, 2),
    );
    assert.equal(
      state.animatedOutletStyle?.flexGrow,
      '1',
      JSON.stringify(state, null, 2),
    );
    assert.ok(
      (state.animatedOutlet?.height ?? 0) >= (state.container?.height ?? 0) - 2,
      JSON.stringify(state, null, 2),
    );
    assert.ok(state.routeContent, JSON.stringify(state, null, 2));
    assert.ok(state.systemLink, JSON.stringify(state, null, 2));
    assert.equal(
      state.externalCoreIcon?.width,
      30,
      JSON.stringify(state, null, 2),
    );
    assert.equal(
      state.externalCoreIcon?.height,
      30,
      JSON.stringify(state, null, 2),
    );
    assert.notEqual(
      state.systemLabelStyle?.textOverflow,
      'ellipsis',
      JSON.stringify(state, null, 2),
    );
    assert.notEqual(
      state.systemLabelStyle?.whiteSpace,
      'nowrap',
      JSON.stringify(state, null, 2),
    );
    assert.equal(
      state.systemDescriptionColor,
      state.zinc500Color,
      JSON.stringify(state, null, 2),
    );

    const evidencePath = process.env.CHIMERA_E2E_EVIDENCE_PATH;
    if (evidencePath) {
      fs.mkdirSync(path.dirname(evidencePath), { recursive: true });
      await browser.saveScreenshot(evidencePath);
    }
  });
});
