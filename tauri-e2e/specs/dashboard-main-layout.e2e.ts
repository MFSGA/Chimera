import assert from 'node:assert/strict';
import fs from 'node:fs';
import path from 'node:path';

const targetPath = '/main/dashboard';

async function invoke<T>(command: string, args?: Record<string, unknown>) {
  return browser.execute(
    async (name, parameters) => {
      const internals = (
        window as typeof window & {
          __TAURI_INTERNALS__: {
            invoke: <R>(
              command: string,
              args?: Record<string, unknown>,
            ) => Promise<R>;
          };
        }
      ).__TAURI_INTERNALS__;
      return internals.invoke<T>(name, parameters);
    },
    command,
    args,
  );
}

/** Create and focus the ref-aligned main window. */
async function openMainWindow() {
  await invoke('create_main_window');
  await browser.waitUntil(
    async () => (await browser.getWindowHandles()).includes('main'),
    { timeout: 15_000, timeoutMsg: 'The main window was not created.' },
  );
  await browser.switchToWindow('main');
}

describe('main dashboard reference layout', () => {
  before(async () => {
    await browser.setWindowSize(1240, 638);
    await browser.execute(() => {
      localStorage.setItem(btoa('paraglide-language-cache'), 'zh-cn');
    });
    await openMainWindow();
    await browser.setWindowSize(1240, 638);

    const link = await $(`a[href="${targetPath}"]`);
    await link.waitForClickable({ timeout: 15_000 });
    await link.click();
    await browser.waitUntil(
      async () =>
        browser.execute(
          (expected) => location.pathname === expected,
          targetPath,
        ),
      { timeout: 15_000, timeoutMsg: 'Dashboard route did not open.' },
    );
  });

  it('uses ref cards, real core artwork, and main settings links', async () => {
    const container = await $('[data-slot="dashboard-widget-container"]');
    await container.waitForDisplayed({ timeout: 15_000 });

    await browser.waitUntil(
      async () =>
        browser.execute(
          () =>
            document.querySelectorAll('[data-slot="widget-sparkline-card"]')
              .length >= 4,
        ),
      { timeout: 15_000, timeoutMsg: 'Dashboard widgets did not render.' },
    );

    const state = await browser.execute(() => {
      const dashboard = document.querySelector<HTMLElement>(
        '[data-slot="dashboard-container"]',
      );
      const container = document.querySelector<HTMLElement>(
        '[data-slot="dashboard-widget-container"]',
      );
      const sparklines = Array.from(
        document.querySelectorAll<HTMLElement>(
          '[data-slot="widget-sparkline-card"]',
        ),
      );
      const currentCore = document.querySelector<HTMLElement>(
        '[data-slot="current-core-card"]',
      );
      const coreIcon = document.querySelector<HTMLImageElement>(
        'img[data-slot="core-icon"]',
      );
      const systemLink = document.querySelector<HTMLAnchorElement>(
        'a[href="/main/settings/system"]',
      );
      const clashLink = document.querySelector<HTMLAnchorElement>(
        'a[href="/main/settings/clash"]',
      );
      const shortcutHeaders = Array.from(
        document.querySelectorAll<HTMLElement>('[data-slot="card-header"]'),
      );
      const firstTitle = document.querySelector<HTMLElement>(
        '[data-slot="widget-sparkline-card-title"]',
      );
      const firstValue = document.querySelector<HTMLElement>(
        '[data-slot="widget-sparkline-card-content"].text-2xl',
      );
      const firstCard = sparklines[0] ?? null;
      const style = (element: HTMLElement | null) => {
        if (!element) return null;
        const computed = getComputedStyle(element);
        return {
          position: computed.position,
          display: computed.display,
          flexDirection: computed.flexDirection,
          overflowX: computed.overflowX,
          overflowY: computed.overflowY,
          paddingTop: computed.paddingTop,
          paddingRight: computed.paddingRight,
          paddingBottom: computed.paddingBottom,
          paddingLeft: computed.paddingLeft,
          gap: computed.gap,
          fontSize: computed.fontSize,
          fontWeight: computed.fontWeight,
          lineHeight: computed.lineHeight,
          width: computed.width,
          height: computed.height,
        };
      };
      const rect = (element: HTMLElement | null) =>
        element
          ? {
              x: Math.round(element.getBoundingClientRect().x),
              y: Math.round(element.getBoundingClientRect().y),
              width: Math.round(element.getBoundingClientRect().width),
              height: Math.round(element.getBoundingClientRect().height),
            }
          : null;

      return {
        dashboard: { rect: rect(dashboard), style: style(dashboard) },
        container: { rect: rect(container), style: style(container) },
        firstCardStyle: style(firstCard),
        firstTitleStyle: style(firstTitle),
        firstValueStyle: style(firstValue),
        sparklineCount: sparklines.length,
        sparklineRects: sparklines.map(rect),
        currentCore: rect(currentCore),
        coreIcon: coreIcon
          ? {
              tagName: coreIcon.tagName,
              src: coreIcon.getAttribute('src') ?? '',
              rect: rect(coreIcon),
            }
          : null,
        hasSystemLink: Boolean(systemLink),
        hasClashLink: Boolean(clashLink),
        shortcutHeaderCount: shortcutHeaders.length,
        viewport: { width: window.innerWidth, height: window.innerHeight },
      };
    });

    assert.ok(state.viewport.width >= 1200, JSON.stringify(state, null, 2));
    assert.ok(state.viewport.height >= 600, JSON.stringify(state, null, 2));
    assert.ok(state.container.rect, JSON.stringify(state, null, 2));
    assert.equal(state.dashboard.style?.position, 'relative');
    assert.equal(state.dashboard.style?.display, 'flex');
    assert.equal(state.dashboard.style?.flexDirection, 'column');
    assert.equal(state.dashboard.style?.overflowX, 'hidden');
    assert.equal(state.dashboard.style?.overflowY, 'hidden');
    assert.deepEqual(
      [
        state.container.style?.paddingTop,
        state.container.style?.paddingRight,
        state.container.style?.paddingBottom,
        state.container.style?.paddingLeft,
      ],
      ['16px', '16px', '16px', '16px'],
      JSON.stringify(state, null, 2),
    );
    assert.equal(state.container.style?.display, 'flex');
    assert.equal(state.container.style?.flexDirection, 'column');
    assert.equal(state.firstCardStyle?.position, 'relative');
    assert.equal(state.firstTitleStyle?.display, 'flex');
    assert.equal(state.firstTitleStyle?.gap, '8px');
    assert.equal(state.firstValueStyle?.fontSize, '24px');
    assert.equal(state.firstValueStyle?.fontWeight, '700');
    assert.equal(state.firstValueStyle?.lineHeight, '32px');
    assert.ok(state.sparklineCount >= 4, JSON.stringify(state, null, 2));
    assert.equal(state.hasSystemLink, true, JSON.stringify(state, null, 2));
    assert.equal(state.hasClashLink, true, JSON.stringify(state, null, 2));
    assert.ok(state.shortcutHeaderCount >= 2, JSON.stringify(state, null, 2));
    assert.ok(state.currentCore, JSON.stringify(state, null, 2));
    assert.equal(
      state.coreIcon?.tagName,
      'IMG',
      JSON.stringify(state, null, 2),
    );
    assert.ok(
      (state.coreIcon?.rect?.width ?? 0) >= 40,
      JSON.stringify(state, null, 2),
    );
    assert.ok(
      (state.coreIcon?.src.length ?? 0) > 0,
      JSON.stringify(state, null, 2),
    );

    const evidencePath = process.env.CHIMERA_E2E_EVIDENCE_PATH;
    if (evidencePath) {
      fs.mkdirSync(path.dirname(evidencePath), { recursive: true });
      await browser.saveScreenshot(evidencePath);
    }
  });
});
