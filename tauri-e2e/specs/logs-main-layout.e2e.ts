import assert from 'node:assert/strict';
import fs from 'node:fs';
import path from 'node:path';

const targetPath = '/main/logs';

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

describe('main logs reference layout', () => {
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
      { timeout: 15_000, timeoutMsg: 'Logs route did not open.' },
    );
  });

  it('uses the ref sidebar, shared scroll area, and bottom search bar', async () => {
    const input = await $('[data-slot="logs-search-input-field"]');
    await input.waitForDisplayed({ timeout: 15_000 });

    const state = await browser.execute(() => {
      const sidebar = document.querySelector<HTMLElement>(
        '[data-slot="slider-sidebar"]',
      );
      const scrollAreas = Array.from(
        document.querySelectorAll<HTMLElement>('[data-slot="scroll-area"]'),
      );
      const search = document.querySelector<HTMLElement>(
        '[data-slot="logs-search"]',
      );
      const inputElement = document.querySelector<HTMLInputElement>(
        '[data-slot="logs-search-input-field"]',
      );
      const emptyState = document.querySelector<HTMLElement>(
        '[data-slot="logs-no-logs"]',
      );
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
        sidebar: rect(sidebar),
        search: rect(search),
        input: rect(inputElement),
        inputPlaceholder: inputElement?.placeholder ?? '',
        emptyState: rect(emptyState),
        scrollAreaCount: scrollAreas.length,
        viewport: { width: innerWidth, height: innerHeight },
      };
    });

    assert.ok(state.viewport.width >= 1200, JSON.stringify(state, null, 2));
    assert.ok(state.viewport.height >= 600, JSON.stringify(state, null, 2));
    assert.equal(state.sidebar?.width, 64, JSON.stringify(state, null, 2));
    assert.equal(state.search?.height, 64, JSON.stringify(state, null, 2));
    assert.equal(state.input?.height, 40, JSON.stringify(state, null, 2));
    assert.ok(
      state.inputPlaceholder.length > 0,
      JSON.stringify(state, null, 2),
    );
    assert.ok(state.scrollAreaCount >= 1, JSON.stringify(state, null, 2));
    assert.ok(state.emptyState, JSON.stringify(state, null, 2));

    const evidencePath = process.env.CHIMERA_E2E_EVIDENCE_PATH;
    if (evidencePath) {
      fs.mkdirSync(path.dirname(evidencePath), { recursive: true });
      await browser.saveScreenshot(evidencePath);
    }
  });
});
