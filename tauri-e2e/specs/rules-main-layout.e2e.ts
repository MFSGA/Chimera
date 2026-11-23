import assert from 'node:assert/strict';
import fs from 'node:fs';
import path from 'node:path';
import { openMainRoute } from './main-window.js';

const targetPath = '/main/rules';

/** Main-window UI verification: real WebDriver navigation, click and typing. */
describe('main rules reference layout', () => {
  beforeEach(async () => {
    await browser.setWindowSize(1240, 638);
    await browser.execute(() => {
      localStorage.setItem(btoa('paraglide-language-cache'), 'en');
    });
    await openMainRoute(targetPath);
    await browser.setWindowSize(1240, 638);
  });

  it('renders ref navigation rail, sortable header, scrollable surface, and docked search', async () => {
    const input = await $('[data-slot="rules-search-input-field"]');
    const header = await $('[data-slot="rules-header"]');
    const sidebar = await $('[data-slot="navigation-sidebar"]');
    await input.waitForDisplayed({ timeout: 15_000 });
    await header.waitForDisplayed({ timeout: 15_000 });
    await sidebar.waitForDisplayed({ timeout: 15_000 });
    // ref defers mounting the virtual list until after the first page render.
    await $('[data-slot="rules-virtual-container"]').waitForDisplayed({
      timeout: 15_000,
    });

    const state = await browser.execute(() => {
      const selector = (slot: string) =>
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
      const page = selector('data-page');
      const dock = selector('data-page-dock');
      const surface = selector('data-page-surface');
      const sidebar = selector('navigation-sidebar');
      const header = selector('rules-header');
      const search = selector('rules-search');
      const field = document.querySelector<HTMLInputElement>(
        '[data-slot="rules-search-input-field"]',
      );
      return {
        page: rect(page),
        pageTitle: page?.getAttribute('aria-label') ?? '',
        sidebar: rect(sidebar),
        surface: rect(surface),
        dock: rect(dock),
        search: rect(search),
        header: rect(header),
        headerText: header?.textContent?.trim() ?? '',
        inputPlaceholder: field?.placeholder ?? '',
        ruleContainer: Boolean(selector('rules-virtual-container')),
        viewports:
          surface?.querySelectorAll('[data-slot="scroll-area-viewport"]')
            .length ?? 0,
      };
    });

    assert.equal(state.sidebar?.width, 88, JSON.stringify(state, null, 2));
    assert.equal(
      state.pageTitle.length > 0,
      true,
      JSON.stringify(state, null, 2),
    );
    assert.ok(
      state.headerText.includes('Connections'),
      JSON.stringify(state, null, 2),
    );
    assert.ok(
      state.headerText.includes('Speed'),
      JSON.stringify(state, null, 2),
    );
    assert.ok(
      state.headerText.includes('Total'),
      JSON.stringify(state, null, 2),
    );
    assert.ok(state.ruleContainer, JSON.stringify(state, null, 2));
    assert.equal(state.viewports, 2, JSON.stringify(state, null, 2));
    assert.ok(
      (state.surface?.y ?? 0) < (state.dock?.y ?? 0),
      JSON.stringify(state, null, 2),
    );
    assert.ok(
      (state.dock?.y ?? 0) < (state.page?.y ?? 0) + (state.page?.height ?? 0),
      JSON.stringify(state, null, 2),
    );
    assert.equal(
      state.inputPlaceholder,
      'Search rules by type, payload, or proxy...',
    );
    assert.ok(state.search, JSON.stringify(state, null, 2));

    const evidencePath = process.env.CHIMERA_E2E_EVIDENCE_PATH;
    if (evidencePath) {
      fs.mkdirSync(path.dirname(evidencePath), { recursive: true });
      await browser.saveScreenshot(evidencePath);
    }
  });

  it('keeps a typed search query in the route and clears it through the actual input', async () => {
    const input = await $('[data-slot="rules-search-input-field"]');
    await input.waitForDisplayed({ timeout: 15_000 });
    await input.setValue('example.com');

    let lastUrl = '';
    try {
      await browser.waitUntil(
        async () => {
          lastUrl = await browser.getUrl();
          return new URL(lastUrl).searchParams.get('q') === 'example.com';
        },
        {
          timeout: 10_000,
          timeoutMsg: 'Rules query was not written into the URL.',
        },
      );
    } catch (cause) {
      throw new Error(`Rules search URL persistence failed: ${lastUrl}`, {
        cause,
      });
    }

    assert.equal(await input.getValue(), 'example.com');

    await input.setValue('');
    try {
      await browser.waitUntil(
        async () => {
          lastUrl = await browser.getUrl();
          return !new URL(lastUrl).searchParams.has('q');
        },
        {
          timeout: 10_000,
          timeoutMsg: 'Clearing Rules search did not clear the URL.',
        },
      );
    } catch (cause) {
      throw new Error(`Rules search clear failed: ${lastUrl}`, { cause });
    }
    assert.equal(await input.getValue(), '');
  });

  it('sorts by Connections and restores rule order through the visible header', async () => {
    const header = await $('[data-slot="rules-header"]');
    await header.waitForDisplayed({ timeout: 15_000 });
    const sortButton = await $('button=Connections');
    await sortButton.waitForDisplayed({ timeout: 15_000 });

    await sortButton.click();
    let lastUrl = '';
    try {
      await browser.waitUntil(
        async () => {
          lastUrl = await browser.getUrl();
          return new URL(lastUrl).searchParams.get('sort') === 'connections';
        },
        {
          timeout: 10_000,
          timeoutMsg: 'Rules sort was not written into the URL.',
        },
      );
    } catch (cause) {
      throw new Error(`Rules sort navigation failed: ${lastUrl}`, { cause });
    }

    await sortButton.click();
    try {
      await browser.waitUntil(
        async () => {
          lastUrl = await browser.getUrl();
          return !new URL(lastUrl).searchParams.has('sort');
        },
        {
          timeout: 10_000,
          timeoutMsg: 'Rules sort did not restore rule order.',
        },
      );
    } catch (cause) {
      throw new Error(`Rules default sort navigation failed: ${lastUrl}`, {
        cause,
      });
    }
  });

  it('expands and collapses the ref sidebar through its user-facing toggle', async () => {
    const toggle = await $('[data-slot="navigation-sidebar-toggle"]');
    await toggle.waitForDisplayed({ timeout: 15_000 });
    assert.equal(await toggle.getAttribute('aria-expanded'), 'false');

    await toggle.click();
    await browser.waitUntil(
      async () => (await toggle.getAttribute('aria-expanded')) === 'true',
      { timeout: 10_000, timeoutMsg: 'Rules sidebar failed to expand.' },
    );

    await toggle.click();
    await browser.waitUntil(
      async () => (await toggle.getAttribute('aria-expanded')) === 'false',
      { timeout: 10_000, timeoutMsg: 'Rules sidebar failed to collapse.' },
    );
  });
});
