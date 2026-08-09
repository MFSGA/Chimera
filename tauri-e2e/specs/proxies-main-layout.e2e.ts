import assert from 'node:assert/strict';
import fs from 'node:fs';
import path from 'node:path';

const targetPath = '/main/proxies';

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

async function openMainWindow() {
  await invoke('create_main_window');
  await browser.waitUntil(
    async () => (await browser.getWindowHandles()).includes('main'),
    { timeout: 15_000, timeoutMsg: 'The main window was not created.' },
  );
  await browser.switchToWindow('main');
}

describe('main proxies reference layout', () => {
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
      { timeout: 15_000, timeoutMsg: 'Proxies route did not open.' },
    );
  });

  it('keeps the ref content hierarchy without an extra search toolbar', async () => {
    const content = await $('[data-slot="proxies-content-scroll-area"]');
    await content.waitForDisplayed({ timeout: 15_000 });

    const state = await browser.execute(() => {
      const container = document.querySelector<HTMLElement>(
        '[data-slot="proxies-container"]',
      );
      const contentArea = document.querySelector<HTMLElement>(
        '[data-slot="proxies-content-scroll-area"]',
      );
      const sidebar = document.querySelector<HTMLElement>(
        '[data-slot="proxies-sidebar-scroll-area"]',
      );
      const empty = document.querySelector<HTMLElement>(
        '[data-slot="proxies-no-proxies"]',
      );
      const search = document.querySelector<HTMLElement>(
        '[data-slot="proxies-search-bar"]',
      );
      const emptyMessage = empty?.querySelector<HTMLElement>(
        '[data-slot="proxies-no-proxies-message"]',
      );
      const emptyIcon = empty?.querySelector<SVGElement>('svg');
      const rect = (element: Element | null) =>
        element
          ? {
              x: Math.round(element.getBoundingClientRect().x),
              y: Math.round(element.getBoundingClientRect().y),
              width: Math.round(element.getBoundingClientRect().width),
              height: Math.round(element.getBoundingClientRect().height),
            }
          : null;

      const style = (element: Element | null) => {
        if (!element) return null;
        const computed = getComputedStyle(element);
        return {
          display: computed.display,
          position: computed.position,
          flexDirection: computed.flexDirection,
          flexGrow: computed.flexGrow,
          flexShrink: computed.flexShrink,
          alignItems: computed.alignItems,
          justifyContent: computed.justifyContent,
          gap: computed.gap,
          overflowX: computed.overflowX,
          overflowY: computed.overflowY,
          fontSize: computed.fontSize,
          lineHeight: computed.lineHeight,
        };
      };

      return {
        container: rect(container),
        containerStyle: style(container),
        content: rect(contentArea),
        contentStyle: style(contentArea),
        sidebar: rect(sidebar),
        empty: rect(empty),
        emptyStyle: style(empty),
        emptyMessageStyle: style(emptyMessage ?? null),
        emptyIcon: rect(emptyIcon ?? null),
        hasSearch: Boolean(search),
        viewport: { width: window.innerWidth, height: window.innerHeight },
      };
    });

    assert.ok(state.viewport.width >= 1200, JSON.stringify(state, null, 2));
    assert.ok(state.viewport.height >= 600, JSON.stringify(state, null, 2));
    assert.ok(state.container, JSON.stringify(state, null, 2));
    assert.ok(state.content, JSON.stringify(state, null, 2));
    assert.equal(state.hasSearch, false, JSON.stringify(state, null, 2));
    assert.equal(state.containerStyle?.display, 'flex');
    assert.equal(state.contentStyle?.display, 'flex');
    assert.equal(state.contentStyle?.position, 'relative');
    assert.equal(state.contentStyle?.flexDirection, 'column');
    assert.equal(state.contentStyle?.flexGrow, '3');
    assert.equal(state.contentStyle?.flexShrink, '1');
    assert.equal(state.contentStyle?.overflowX, 'clip');
    assert.equal(state.contentStyle?.overflowY, 'clip');

    if (state.empty) {
      assert.equal(state.empty.width, state.content?.width);
      assert.equal(state.empty.height, state.content?.height);
      assert.equal(state.emptyStyle?.display, 'flex');
      assert.equal(state.emptyStyle?.position, 'absolute');
      assert.equal(state.emptyStyle?.flexDirection, 'column');
      assert.equal(state.emptyStyle?.alignItems, 'center');
      assert.equal(state.emptyStyle?.justifyContent, 'center');
      assert.equal(state.emptyStyle?.gap, '16px');
      assert.equal(state.emptyMessageStyle?.fontSize, '14px');
      assert.equal(state.emptyMessageStyle?.lineHeight, '20px');
      assert.equal(state.emptyIcon?.width, 64);
      assert.equal(state.emptyIcon?.height, 64);
    } else {
      assert.ok(state.sidebar, JSON.stringify(state, null, 2));
      assert.ok(
        (state.sidebar?.width ?? 0) >= 240,
        JSON.stringify(state, null, 2),
      );
    }

    const evidencePath = process.env.CHIMERA_E2E_EVIDENCE_PATH;
    if (evidencePath) {
      fs.mkdirSync(path.dirname(evidencePath), { recursive: true });
      await browser.saveScreenshot(evidencePath);
    }
  });

  it('opens the profile list from the empty-state action', async () => {
    const addProfile = await $('[data-slot="proxies-no-proxies-button"]');
    await addProfile.waitForClickable({ timeout: 15_000 });

    const href = await addProfile.getAttribute('href');
    assert.equal(href, '/main/profiles/profile');

    await addProfile.click();
    await browser.waitUntil(
      async () =>
        browser.execute(() => location.pathname === '/main/profiles/profile'),
      {
        timeout: 15_000,
        timeoutMsg: 'The empty-state action did not open the profile list.',
      },
    );
  });
});
