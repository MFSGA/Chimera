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

  it('shows the navigation rail, logs viewport and floating search actions', async () => {
    const input = await $('[data-slot="logs-search-input-field"]');
    await input.waitForDisplayed({ timeout: 15_000 });

    const rail = await $('[data-slot="navigation-sidebar"]');
    const toolbar = await $('[data-slot="logs-toolbar"]');
    const scrollArea = await $('[data-slot="logs-scroll-area"]');
    const clearLogs = await $('[data-slot="logs-clear-button"]');

    assert.equal(await rail.isDisplayed(), true);
    assert.equal(await toolbar.isDisplayed(), true);
    assert.equal(await scrollArea.isDisplayed(), true);
    assert.equal(await clearLogs.isExisting(), true);

    const state = await browser.execute(() => {
      const rect = (selector: string) => {
        const element = document.querySelector<HTMLElement>(selector);
        if (!element) return null;
        const bounds = element.getBoundingClientRect();
        return {
          x: Math.round(bounds.x),
          y: Math.round(bounds.y),
          width: Math.round(bounds.width),
          height: Math.round(bounds.height),
          bottom: Math.round(bounds.bottom),
        };
      };
      return {
        rail: rect('[data-slot="navigation-sidebar"]'),
        toolbar: rect('[data-slot="logs-toolbar"]'),
        dock: rect('[data-slot="data-page-dock"]'),
        input: rect('[data-slot="logs-search-input-field"]'),
        emptyState: rect('[data-slot="logs-no-logs"]'),
        virtualList: rect('[data-slot="logs-virtual-list"]'),
        viewport: { width: innerWidth, height: innerHeight },
      };
    });

    assert.ok(state.viewport.width >= 1200, JSON.stringify(state));
    assert.ok(state.viewport.height >= 600, JSON.stringify(state));
    assert.equal(state.rail?.width, 88, JSON.stringify(state));
    assert.ok((state.input?.height ?? 0) >= 36, JSON.stringify(state));
    assert.ok(
      state.dock &&
        state.toolbar &&
        state.toolbar.bottom <= state.dock.bottom &&
        state.dock.bottom <= state.viewport.height,
      JSON.stringify(state),
    );
    assert.ok(state.emptyState || state.virtualList, JSON.stringify(state));

    const evidencePath = process.env.CHIMERA_E2E_EVIDENCE_PATH;
    if (evidencePath) {
      fs.mkdirSync(path.dirname(evidencePath), { recursive: true });
      await browser.saveScreenshot(evidencePath);
    }
  });

  it('filters seven log levels through visible navigation links', async () => {
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
    const fatalHref = await fatalFilter.getAttribute('href');
    assert.ok(fatalHref, 'Fatal level must be a navigable link.');
    const fatalUrl = new URL(fatalHref, 'http://tauri.localhost');
    assert.equal(fatalUrl.pathname, targetPath);
    assert.equal(fatalUrl.searchParams.get('level'), 'fatal');

    await fatalFilter.waitForClickable({ timeout: 15_000 });
    await fatalFilter.click();
    await browser.waitUntil(
      async () =>
        browser.execute(() => {
          const selected = document.querySelector<HTMLElement>(
            '[data-slot="logs-level-filter"][data-log-level="fatal"]',
          );
          return (
            location.pathname === '/main/logs' &&
            new URLSearchParams(location.search).get('level') === 'fatal' &&
            selected?.getAttribute('data-active') === 'true'
          );
        }),
      {
        timeout: 15_000,
        timeoutMsg: 'Fatal filter left the logs route or was not selected.',
      },
    );

    const allFilter = await $(
      '[data-slot="logs-level-filter"][data-log-level="all"]',
    );
    await allFilter.waitForClickable({ timeout: 15_000 });
    await allFilter.click();
    await browser.waitUntil(
      async () =>
        browser.execute(() => {
          const selected = document.querySelector<HTMLElement>(
            '[data-slot="logs-level-filter"][data-log-level="all"]',
          );
          return (
            location.pathname === '/main/logs' &&
            new URLSearchParams(location.search).get('level') === null &&
            selected?.getAttribute('data-active') === 'true'
          );
        }),
      {
        timeout: 15_000,
        timeoutMsg: 'All levels filter was not restored on the logs route.',
      },
    );
  });

  it('allows a user to type and clear a log search without reloading', async () => {
    const input = await $('[data-slot="logs-search-input-field"]');
    await input.waitForDisplayed({ timeout: 15_000 });
    await input.setValue('test-logs-search');
    await browser.waitUntil(
      async () => (await input.getValue()) === 'test-logs-search',
      { timeout: 5_000 },
    );

    const clear = await $('[data-slot="search-field-clear"]');
    await clear.waitForClickable({ timeout: 5_000 });
    await clear.click();
    await browser.waitUntil(async () => (await input.getValue()) === '', {
      timeout: 5_000,
      timeoutMsg: 'Clicking the visible clear control did not reset search.',
    });
    assert.equal(
      await browser.getUrl().then((url) => new URL(url).pathname),
      targetPath,
    );
  });
});
