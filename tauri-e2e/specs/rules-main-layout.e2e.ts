import assert from 'node:assert/strict';
import fs from 'node:fs';
import path from 'node:path';
import { openMainRoute } from './main-window.js';

const targetPath = '/main/rules';

describe('main rules reference layout', () => {
  beforeEach(async () => {
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

  it('opens the ref tooltip for a focused collapsed sidebar item', async () => {
    const triggerSelector =
      '[data-slot="rules-proxy-item"][data-proxy-item-kind="all"]';
    const triggers = await $$(triggerSelector);
    assert.equal(
      triggers.length,
      1,
      'The all-proxies sidebar link selector must identify exactly one item.',
    );
    const trigger = await $(triggerSelector);
    await trigger.waitForDisplayed({ timeout: 15_000 });

    assert.equal(
      await trigger.isFocused(),
      false,
      'The tooltip trigger must start unfocused.',
    );
    const focused = await browser.execute((selector) => {
      const target = document.querySelector<HTMLElement>(selector);
      if (!target) return false;
      target.focus();
      return document.activeElement === target;
    }, triggerSelector);
    assert.equal(
      focused,
      true,
      'The all-proxies sidebar link did not receive focus.',
    );

    const inspectTooltip = () =>
      browser.execute((selector) => {
        const trigger = document.querySelector<HTMLElement>(selector);
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
          focused: !!trigger && document.activeElement === trigger,
          trigger: rect(trigger),
          tooltip: rect(tooltip),
          triggerText: trigger?.textContent?.trim() ?? '',
          tooltipText: tooltip?.textContent?.trim() ?? '',
          tooltipSide: tooltip?.dataset.side ?? '',
        };
      }, triggerSelector);

    let state = await inspectTooltip();
    try {
      await browser.waitUntil(
        async () => {
          state = await inspectTooltip();
          return (
            state.focused &&
            !!state.tooltip &&
            state.tooltipText === state.triggerText
          );
        },
        { timeout: 15_000, timeoutMsg: 'Rules sidebar tooltip did not open.' },
      );
    } catch (error) {
      try {
        state = await inspectTooltip();
      } catch (diagnosticError) {
        throw new Error(
          `Rules sidebar tooltip did not open; last observed state: ${JSON.stringify(state)}; final diagnostic failed: ${String(diagnosticError)}`,
          { cause: error },
        );
      }
      throw new Error(
        `Rules sidebar tooltip did not open; final state: ${JSON.stringify(state)}`,
        { cause: error },
      );
    }

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
