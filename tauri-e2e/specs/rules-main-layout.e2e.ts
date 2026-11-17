import assert from 'node:assert/strict';
import fs from 'node:fs';
import path from 'node:path';
import { openMainRoute } from './main-window.js';

const targetPath = '/main/rules';

/**
 * User-visible Rules UI contract after the pinned ref navigation and DataPage
 * migration. Runtime stats require a separate backend API and are not claimed
 * here. All interactions go through WebDriver, not DOM-dispatched clicks.
 */
describe('main rules reference layout', () => {
  beforeEach(async () => {
    await browser.setWindowSize(1240, 638);
    await browser.execute(() => {
      localStorage.setItem(btoa('paraglide-language-cache'), 'zh-cn');
    });
    await openMainRoute(targetPath);
    await browser.setWindowSize(1240, 638);
  });

  it('renders the ref navigation rail, DataPage, and floating search', async () => {
    const input = await $('[data-slot="rules-search-input-field"]');
    await input.waitForDisplayed({ timeout: 15_000 });

    const sidebar = await $('[data-slot="navigation-sidebar"]');
    await sidebar.waitForDisplayed({ timeout: 15_000 });
    await browser.waitUntil(
      async () => Math.abs((await sidebar.getSize('width')) - 88) <= 2,
      {
        timeout: 15_000,
        timeoutMsg: 'Rules rail did not settle to the ref width.',
      },
    );

    const state = await browser.execute(() => {
      const get = (slot: string) =>
        document.querySelector<HTMLElement>(`[data-slot="${slot}"]`);
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
        rail: rect(get('navigation-sidebar')),
        page: rect(get('data-page')),
        toolbar: rect(get('rules-search')),
        search: rect(get('search-field')),
        header: rect(get('rules-header')),
        table: rect(get('rules-virtual-table')),
        empty: rect(get('rules-empty')),
        scrollAreaCount: document.querySelectorAll('[data-slot="scroll-area"]')
          .length,
        viewport: { width: innerWidth, height: innerHeight },
      };
    });

    assert.ok(state.viewport.width >= 1200, JSON.stringify(state));
    assert.ok(state.viewport.height >= 600, JSON.stringify(state));
    assert.equal(state.rail?.width, 88, JSON.stringify(state));
    assert.ok(state.page && state.page.height > 0, JSON.stringify(state));
    assert.ok(state.toolbar && state.toolbar.height > 0, JSON.stringify(state));
    assert.ok(state.search && state.search.height > 0, JSON.stringify(state));
    assert.ok(
      state.header && state.header.height === 40,
      JSON.stringify(state),
    );
    assert.ok(state.scrollAreaCount >= 2, JSON.stringify(state));
    assert.ok(
      state.table || state.empty,
      `Rules must render a real data table or an empty state: ${JSON.stringify(state)}`,
    );

    const evidencePath = process.env.CHIMERA_E2E_EVIDENCE_PATH;
    if (evidencePath) {
      fs.mkdirSync(path.dirname(evidencePath), { recursive: true });
      await browser.saveScreenshot(evidencePath);
    }
  });

  it('expands and collapses the ref rail with the visible toggle', async () => {
    const toggle = await $('[data-slot="navigation-sidebar-toggle"]');
    await toggle.waitForDisplayed({ timeout: 15_000 });

    assert.equal(await toggle.getAttribute('aria-expanded'), 'false');
    await toggle.click();
    await browser.waitUntil(
      async () => (await toggle.getAttribute('aria-expanded')) === 'true',
      { timeout: 15_000, timeoutMsg: 'Rules rail did not expand.' },
    );

    const allGroups = await $(
      '[data-slot="rules-proxy-item"][data-proxy-item-kind="all"]',
    );
    await allGroups.waitForDisplayed({ timeout: 15_000 });
    assert.equal(await allGroups.getAttribute('data-active'), 'true');

    await toggle.click();
    await browser.waitUntil(
      async () => (await toggle.getAttribute('aria-expanded')) === 'false',
      { timeout: 15_000, timeoutMsg: 'Rules rail did not collapse.' },
    );
  });

  it('searches rules, updates the URL, and clears through the real UI', async () => {
    const input = await $('[data-slot="rules-search-input-field"]');
    await input.waitForDisplayed({ timeout: 15_000 });

    const query = 'unlikely-rule-search-e2e-17';
    await input.setValue(query);

    await browser.waitUntil(
      async () =>
        browser.execute(
          (term) => new URLSearchParams(location.search).get('q') === term,
          query,
        ),
      { timeout: 15_000, timeoutMsg: 'Rules search was not saved to the URL.' },
    );

    const empty = await $('[data-slot="rules-empty"]');
    await empty.waitForDisplayed({ timeout: 15_000 });

    const clear = await $('[data-slot="search-field-clear"]');
    await clear.waitForDisplayed({ timeout: 15_000 });
    await clear.click();

    await browser.waitUntil(
      async () =>
        browser.execute(() => !new URLSearchParams(location.search).has('q')),
      { timeout: 15_000, timeoutMsg: 'Clear did not remove the Rules query.' },
    );
    assert.equal(await input.getValue(), '');
  });
});
