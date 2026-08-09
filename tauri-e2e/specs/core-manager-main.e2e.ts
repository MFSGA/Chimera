import assert from 'node:assert/strict';
import fs from 'node:fs';
import path from 'node:path';

const targetPath = '/main/settings/clash';

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

async function waitForPath(pathname: string) {
  await browser.waitUntil(
    async () =>
      browser.execute((expected) => location.pathname === expected, pathname),
    {
      timeout: 15_000,
      timeoutMsg: `Navigation to ${pathname} did not complete.`,
    },
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

describe('main clash core manager reference layout', () => {
  before(async () => {
    await browser.setWindowSize(1240, 638);
    await browser.execute(() => {
      localStorage.setItem(btoa('paraglide-language-cache'), 'zh-cn');
    });
    await openMainWindow();
    await browser.setWindowSize(1240, 638);

    await browser.execute((target) => {
      history.pushState({}, '', target);
      window.dispatchEvent(new PopStateEvent('popstate'));
    }, targetPath);
    await waitForPath(targetPath);
  });

  it('renders ref-style core cards without the legacy MUI core item', async () => {
    const card = await $('[data-slot="core-manager-card"]');
    await card.waitForDisplayed({ timeout: 30_000 });

    await browser.waitUntil(
      async () =>
        browser.execute(
          () =>
            document.querySelectorAll('[data-slot="core-manager-item"]')
              .length > 0,
        ),
      {
        timeout: 30_000,
        timeoutMsg: 'The available core cards did not render.',
      },
    );

    const state = await browser.execute(() => {
      const card = document.querySelector<HTMLElement>(
        '[data-slot="core-manager-card"]',
      );
      const current = document.querySelector<HTMLElement>(
        '[data-slot="core-manager-current"]',
      );
      const content = document.querySelector<HTMLElement>(
        '[data-slot="core-manager-card-content"]',
      );
      const items = Array.from(
        document.querySelectorAll<HTMLElement>(
          '[data-slot="core-manager-item"]',
        ),
      );
      const currentImage =
        current?.querySelector<HTMLImageElement>('img') ?? null;
      const legacyMuiNodes = card
        ? Array.from(card.querySelectorAll<HTMLElement>('*')).filter(
            (element) =>
              Array.from(element.classList).some((name) =>
                name.startsWith('Mui'),
              ),
          )
        : [];
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
        viewport: { width: innerWidth, height: innerHeight },
        card: rect(card),
        content: rect(content),
        current: rect(current),
        currentImage: currentImage?.getAttribute('src') ?? null,
        itemCount: items.length,
        itemRects: items.map((item) => rect(item)),
        legacyMuiCount: legacyMuiNodes.length,
      };
    });

    assert.equal(state.viewport.width, 1224, JSON.stringify(state, null, 2));
    assert.equal(state.viewport.height, 629, JSON.stringify(state, null, 2));
    assert.ok(state.card, JSON.stringify(state, null, 2));
    assert.ok(state.content, JSON.stringify(state, null, 2));
    assert.ok(state.current, JSON.stringify(state, null, 2));
    assert.ok(
      (state.current?.height ?? 0) >= 80,
      JSON.stringify(state, null, 2),
    );
    assert.ok(
      (state.current?.width ?? 0) >= 400,
      JSON.stringify(state, null, 2),
    );
    assert.ok(state.currentImage, JSON.stringify(state, null, 2));
    assert.ok(state.itemCount >= 1, JSON.stringify(state, null, 2));
    assert.ok(
      state.itemRects.every((rect) => (rect?.height ?? 0) >= 48),
      JSON.stringify(state, null, 2),
    );
    assert.equal(state.legacyMuiCount, 0, JSON.stringify(state, null, 2));

    const evidencePath = process.env.CHIMERA_E2E_EVIDENCE_PATH;
    if (evidencePath) {
      fs.mkdirSync(path.dirname(evidencePath), { recursive: true });
      await browser.saveScreenshot(evidencePath);
    }
  });
});
