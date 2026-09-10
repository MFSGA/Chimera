import assert from 'node:assert/strict';
import fs from 'node:fs';
import path from 'node:path';
import { openMainRoute } from './main-window.js';

const targetPath = '/main/rules';

describe('main rules reference layout', () => {
  before(async () => {
    await browser.setWindowSize(1240, 638);
    await browser.execute(() => {
      localStorage.setItem(btoa('paraglide-language-cache'), 'zh-cn');
    });
    await openMainRoute(targetPath);
    await browser.setWindowSize(1240, 638);
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
    await browser.execute((element) => {
      element.dispatchEvent(
        new PointerEvent('pointermove', {
          bubbles: true,
          pointerType: 'mouse',
        }),
      );
    }, firstItem);
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
