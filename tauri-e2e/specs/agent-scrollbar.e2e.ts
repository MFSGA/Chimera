import assert from 'node:assert/strict';
import fs from 'node:fs';
import path from 'node:path';

const targetPath = '/main/assistant';

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

describe('network assistant scrollbar', () => {
  before(async () => {
    await browser.setWindowSize(1240, 638);
    await browser.execute(() => {
      localStorage.setItem(btoa('paraglide-language-cache'), 'zh-cn');
    });
    await openMainWindow();
    await browser.setWindowSize(1240, 638);

    await browser.url(`http://tauri.localhost${targetPath}`);
    await browser.waitUntil(
      async () =>
        browser.execute(
          (expected) => location.pathname === expected,
          targetPath,
        ),
      { timeout: 15_000, timeoutMsg: 'Network assistant route did not open.' },
    );

    const diagnosticButton = await $(
      '[data-slot="agent-page-scroll-area"] button',
    );
    await diagnosticButton.waitForClickable({ timeout: 15_000 });
    await diagnosticButton.click();
  });

  it('keeps overflowing diagnostics scrollable with a visible thumb', async () => {
    await browser.waitUntil(
      async () =>
        browser.execute(() => {
          const viewport = document.querySelector<HTMLElement>(
            '[data-slot="agent-page-scroll-area"] [data-slot="scroll-area-viewport"]',
          );
          return Boolean(
            viewport && viewport.scrollHeight > viewport.clientHeight,
          );
        }),
      {
        timeout: 30_000,
        timeoutMsg: 'Network assistant content did not become scrollable.',
      },
    );

    const state = await browser.execute(() => {
      const root = document.querySelector<HTMLElement>(
        '[data-slot="agent-page-scroll-area"]',
      );
      const viewport = root?.querySelector<HTMLElement>(
        '[data-slot="scroll-area-viewport"]',
      );
      const scrollbar = root?.querySelector<HTMLElement>(
        '[data-slot="scroll-area-scrollbar"]',
      );
      const thumb =
        scrollbar?.firstElementChild instanceof HTMLElement
          ? scrollbar.firstElementChild
          : null;

      if (!root || !viewport || !scrollbar || !thumb) {
        return {
          hasRoot: Boolean(root),
          hasViewport: Boolean(viewport),
          hasScrollbar: Boolean(scrollbar),
          hasThumb: Boolean(thumb),
          scrollbarHtml: scrollbar?.outerHTML.slice(0, 500),
          scrollHeight: 0,
          clientHeight: 0,
          scrollTop: 0,
        };
      }

      const maxScrollTop = viewport.scrollHeight - viewport.clientHeight;
      viewport.scrollTop = Math.min(240, maxScrollTop);

      return {
        hasRoot: true,
        hasViewport: true,
        hasScrollbar: true,
        hasThumb: true,
        scrollHeight: viewport.scrollHeight,
        clientHeight: viewport.clientHeight,
        scrollTop: viewport.scrollTop,
        scrollbarState: scrollbar.dataset.state,
        scrollbarOpacity: getComputedStyle(scrollbar).opacity,
        thumbHeight: thumb.getBoundingClientRect().height,
      };
    });

    assert.ok(state.hasRoot, JSON.stringify(state));
    assert.ok(state.hasViewport, JSON.stringify(state));
    assert.ok(state.hasScrollbar, JSON.stringify(state));
    assert.ok(state.hasThumb, JSON.stringify(state));

    if (
      !('scrollHeight' in state) ||
      !('clientHeight' in state) ||
      !('scrollTop' in state)
    ) {
      assert.fail(JSON.stringify(state));
    }

    assert.ok(state.scrollHeight > state.clientHeight, JSON.stringify(state));
    assert.ok(state.scrollTop > 0, JSON.stringify(state));
    assert.equal(state.scrollbarState, 'visible', JSON.stringify(state));
    assert.equal(state.scrollbarOpacity, '1', JSON.stringify(state));
    assert.ok(state.thumbHeight > 0, JSON.stringify(state));

    const evidencePath = process.env.CHIMERA_E2E_EVIDENCE_PATH;
    if (evidencePath) {
      fs.mkdirSync(path.dirname(evidencePath), { recursive: true });
      await browser.saveScreenshot(evidencePath);
    }
  });
});
