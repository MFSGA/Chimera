import assert from 'node:assert/strict';
import fs from 'node:fs';
import path from 'node:path';
import { openMainRoute } from './main-window.js';

const targetPath = '/main/logs';

describe('main logs reference layout', () => {
  before(async () => {
    await browser.setWindowSize(1240, 638);
    await browser.execute(() => {
      localStorage.setItem(btoa('paraglide-language-cache'), 'zh-cn');
    });
    await openMainRoute(targetPath);
    await browser.setWindowSize(1240, 638);
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
      const virtualList = document.querySelector<HTMLElement>(
        '[data-slot="logs-virtual-list"]',
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
        virtualList: rect(virtualList),
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
    assert.ok(
      state.emptyState || state.virtualList,
      JSON.stringify(state, null, 2),
    );

    const evidencePath = process.env.CHIMERA_E2E_EVIDENCE_PATH;
    if (evidencePath) {
      fs.mkdirSync(path.dirname(evidencePath), { recursive: true });
      await browser.saveScreenshot(evidencePath);
    }
  });

  it('filters seven log levels via visible sidebar links', async () => {
    const levels = [
      'trace',
      'debug',
      'info',
      'warning',
      'error',
      'fatal',
      'unknown',
    ] as const;

    for (const level of levels) {
      const entry = await $(
        `[data-slot="logs-level-filter"][data-log-level="${level}"]`,
      );
      assert.equal(await entry.isExisting(), true, `Missing ${level} filter`);
    }

    const fatalFilter = await $(
      '[data-slot="logs-level-filter"][data-log-level="fatal"]',
    );
    await fatalFilter.waitForClickable({ timeout: 15_000 });
    await fatalFilter.click();
    await browser.waitUntil(
      async () => (await fatalFilter.getAttribute('data-active')) === 'true',
      { timeout: 15_000, timeoutMsg: 'Fatal filter was not selected.' },
    );

    const selected = await browser.execute(() => ({
      pathname: location.pathname,
      level: new URLSearchParams(location.search).get('level'),
    }));
    assert.equal(selected.pathname, targetPath);
    assert.equal(selected.level, 'fatal');

    const allFilter = await $(
      '[data-slot="logs-level-filter"][data-log-level="all"]',
    );
    await allFilter.waitForClickable({ timeout: 15_000 });
    await allFilter.click();
    await browser.waitUntil(
      async () => (await allFilter.getAttribute('data-active')) === 'true',
      { timeout: 15_000, timeoutMsg: 'All levels filter was not restored.' },
    );
    const clearedLevel = await browser.execute(
      () => new URLSearchParams(location.search).get('level'),
    );
    assert.equal(clearedLevel, null);
  });
});
