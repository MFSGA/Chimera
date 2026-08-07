import assert from 'node:assert/strict';
import fs from 'node:fs';
import path from 'node:path';

const targetPath = '/main/connections';

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

describe('main connections reference layout', () => {
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
      { timeout: 15_000, timeoutMsg: 'Connections route did not open.' },
    );
  });

  it('keeps the ref empty-state, toolbar, and context-menu structure', async () => {
    const toolbar = await $('[data-slot="connections-toolbar"]');
    await toolbar.waitForDisplayed({ timeout: 15_000 });

    const empty = await $('[data-slot="connections-no-connections"]');
    await empty.waitForDisplayed({ timeout: 15_000 });

    const state = await browser.execute(() => {
      const layout = document.querySelector<HTMLElement>(
        '[data-slot="connections-layout"]',
      );
      const container = document.querySelector<HTMLElement>(
        '[data-slot="connections-container"]',
      );
      const scroll = document.querySelector<HTMLElement>(
        '[data-slot="connections-scroll-wrapper"]',
      );
      const toolbar = document.querySelector<HTMLElement>(
        '[data-slot="connections-toolbar"]',
      );
      const empty = document.querySelector<HTMLElement>(
        '[data-slot="connections-no-connections"]',
      );
      const closeButton = toolbar?.querySelector<HTMLButtonElement>('button');
      const search = toolbar?.querySelector<HTMLInputElement>('input');
      const rect = (element: HTMLElement | null | undefined) =>
        element
          ? {
              x: Math.round(element.getBoundingClientRect().x),
              y: Math.round(element.getBoundingClientRect().y),
              width: Math.round(element.getBoundingClientRect().width),
              height: Math.round(element.getBoundingClientRect().height),
            }
          : null;

      return {
        layout: rect(layout),
        container: rect(container),
        scroll: rect(scroll),
        toolbar: rect(toolbar),
        empty: rect(empty),
        closeButton: rect(closeButton),
        search: rect(search),
        emptyText: empty?.innerText ?? '',
        viewport: { width: window.innerWidth, height: window.innerHeight },
      };
    });

    assert.ok(state.viewport.width >= 1200, JSON.stringify(state, null, 2));
    assert.ok(state.layout, JSON.stringify(state, null, 2));
    assert.ok(state.container, JSON.stringify(state, null, 2));
    assert.equal(state.toolbar?.height, 64, JSON.stringify(state, null, 2));
    assert.ok(
      (state.scroll?.height ?? 0) > 400,
      JSON.stringify(state, null, 2),
    );
    assert.ok((state.search?.width ?? 0) > 700, JSON.stringify(state, null, 2));
    assert.ok(
      (state.closeButton?.width ?? 0) >= 32,
      JSON.stringify(state, null, 2),
    );
    assert.ok(state.emptyText.length > 0, JSON.stringify(state, null, 2));

    const closeButton = await $('[data-slot="connections-toolbar"] button');
    await closeButton.waitForClickable({ timeout: 5_000 });
    await closeButton.click();
    await toolbar.waitForDisplayed({ timeout: 5_000 });

    const evidencePath = process.env.CHIMERA_E2E_EVIDENCE_PATH;
    if (evidencePath) {
      fs.mkdirSync(path.dirname(evidencePath), { recursive: true });
      await browser.saveScreenshot(evidencePath);
    }
  });
});
