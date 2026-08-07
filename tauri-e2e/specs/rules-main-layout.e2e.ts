import assert from 'node:assert/strict';
import fs from 'node:fs';
import path from 'node:path';

const targetPath = '/main/rules';

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

describe('main rules reference layout', () => {
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
      { timeout: 15_000, timeoutMsg: 'Rules route did not open.' },
    );
  });

  it('uses the ref sidebar, shared scroll area, and bottom search bar', async () => {
    const input = await $('[data-slot="rules-search-input-field"]');
    await input.waitForDisplayed({ timeout: 15_000 });

    const state = await browser.execute(() => {
      const sidebar = document.querySelector<HTMLElement>(
        '[data-slot="slider-sidebar"]',
      );
      const scrollAreas = Array.from(
        document.querySelectorAll<HTMLElement>('[data-slot="scroll-area"]'),
      );
      const search = document.querySelector<HTMLElement>(
        '[data-slot="rules-search"]',
      );
      const inputElement = document.querySelector<HTMLInputElement>(
        '[data-slot="rules-search-input-field"]',
      );
      const table = document.querySelector<HTMLElement>(
        '[data-slot="rules-virtual-table"]',
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
        table: rect(table),
        scrollAreaCount: scrollAreas.length,
        viewport: { width: innerWidth, height: innerHeight },
      };
    });

    assert.ok(state.viewport.width >= 1200, JSON.stringify(state, null, 2));
    assert.ok(state.viewport.height >= 600, JSON.stringify(state, null, 2));
    assert.equal(state.sidebar?.width, 64, JSON.stringify(state, null, 2));
    assert.equal(state.search?.height, 64, JSON.stringify(state, null, 2));
    assert.equal(state.input?.height, 40, JSON.stringify(state, null, 2));
    assert.equal(
      state.inputPlaceholder,
      'Search rules (type, payload, or proxy)...',
      JSON.stringify(state, null, 2),
    );
    assert.ok(state.scrollAreaCount >= 2, JSON.stringify(state, null, 2));
    assert.ok(state.table, JSON.stringify(state, null, 2));

    const evidencePath = process.env.CHIMERA_E2E_EVIDENCE_PATH;
    if (evidencePath) {
      fs.mkdirSync(path.dirname(evidencePath), { recursive: true });
      await browser.saveScreenshot(evidencePath);
    }
  });

  it('uses the main ref tooltip for collapsed sidebar items', async () => {
    const sidebar = await $('[data-slot="slider-sidebar"]');
    const firstItem = await sidebar.$('a');
    await firstItem.waitForDisplayed({ timeout: 15_000 });
    await browser.execute(() => {
      document
        .querySelector<HTMLElement>('[data-slot="slider-sidebar"] a')
        ?.focus();
    });
    await browser.waitUntil(
      async () =>
        browser.execute(() =>
          Boolean(document.querySelector<HTMLElement>('[role="tooltip"]')),
        ),
      { timeout: 15_000, timeoutMsg: 'Rules sidebar tooltip did not open.' },
    );

    const state = await browser.execute(() => {
      const trigger = document.querySelector<HTMLElement>(
        '[data-slot="slider-sidebar"] a',
      );
      const tooltip = document.querySelector<HTMLElement>('[role="tooltip"]');
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
        trigger: rect(trigger),
        tooltip: rect(tooltip),
        triggerText: trigger?.textContent?.trim() ?? '',
        tooltipText: tooltip?.textContent?.trim() ?? '',
        tooltipSide: tooltip?.dataset.side ?? '',
      };
    });

    assert.ok(state.trigger, JSON.stringify(state, null, 2));
    assert.ok(state.tooltip, JSON.stringify(state, null, 2));
    assert.ok(state.triggerText.length > 0, JSON.stringify(state, null, 2));
    assert.equal(
      state.tooltipText,
      state.triggerText,
      JSON.stringify(state, null, 2),
    );
    assert.equal(state.tooltipSide, 'right', JSON.stringify(state, null, 2));
    assert.ok(
      (state.tooltip?.x ?? 0) >
        (state.trigger?.x ?? 0) + (state.trigger?.width ?? 0),
      JSON.stringify(state, null, 2),
    );

    const evidencePath = process.env.CHIMERA_E2E_EVIDENCE_PATH;
    if (evidencePath) {
      fs.mkdirSync(path.dirname(evidencePath), { recursive: true });
      await browser.saveScreenshot(evidencePath);
    }
  });
});
