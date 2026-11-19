import assert from 'node:assert/strict';
import fs from 'node:fs';
import path from 'node:path';
import { displayedElement, focusElement } from './interaction.js';
import { openMainRoute } from './main-window.js';

const targetPath = '/main/connections';

describe('main connections reference layout', () => {
  before(async () => {
    await browser.setWindowSize(1240, 638);
    await browser.execute(() => {
      localStorage.setItem(btoa('paraglide-language-cache'), 'zh-cn');
    });
  });

  beforeEach(async () => {
    try {
      await openMainRoute(targetPath);
    } catch (error) {
      const state = await browser.execute(() => {
        const root = document.getElementById('root');
        return {
          url: location.href,
          title: document.title,
          readyState: document.readyState,
          rootChildCount: root?.childElementCount ?? null,
          rootText: root?.innerText.slice(0, 200) ?? null,
          appClass: document.documentElement.className,
          appRoot: document.querySelector('[data-slot="app-root"]') !== null,
          scripts: Array.from(document.scripts).map((script) => script.src),
        };
      });
      console.error('Main app startup diagnostic:', JSON.stringify(state));
      throw error;
    }
    const search = await displayedElement('[data-slot="connections-search"]');
    if (await search.getValue()) {
      await search.clearValue();
    }
    await browser.waitUntil(
      async () =>
        browser.execute(
          () =>
            !location.search.includes('q=') &&
            document
              .querySelector(
                '[data-slot="connections-scope-tab"][data-scope="active"]',
              )
              ?.getAttribute('data-state') === 'on',
        ),
      { timeout: 10_000, timeoutMsg: 'Connections did not reset to Active.' },
    );
    await browser.setWindowSize(1240, 638);
  });

  it('keeps the ref empty-state, toolbar, and context-menu structure', async () => {
    const search = await displayedElement('[data-slot="connections-search"]');
    await focusElement(search);
    await browser.keys('__chimera_e2e_no_matching_connection__');

    await browser.waitUntil(
      async () =>
        browser.execute(() => {
          const empty = document.querySelector<HTMLElement>(
            '[data-slot="connections-no-connections"]',
          );
          return Boolean(empty?.innerText.trim());
        }),
      {
        timeout: 15_000,
        timeoutMsg: 'The filtered Connections empty state did not render.',
      },
    );

    const state = await browser.execute(() => {
      const layout = document.querySelector<HTMLElement>(
        '[data-slot="connections-layout"]',
      );
      const container = document.querySelector<HTMLElement>(
        '[data-connections-container="true"]',
      );
      const scroll = document.querySelector<HTMLElement>(
        '[data-slot="connections-scroll-wrapper"]',
      );
      const toolbar = document.querySelector<HTMLElement>(
        '[data-slot="connections-toolbar"]',
      );
      const page = document.querySelector<HTMLElement>(
        '[data-slot="data-page"]',
      );
      const surface = document.querySelector<HTMLElement>(
        '[data-slot="data-page-surface"]',
      );
      const dock = document.querySelector<HTMLElement>(
        '[data-slot="data-page-dock"]',
      );
      const actions = document.querySelector<HTMLElement>(
        '[data-slot="connections-actions"]',
      );
      const scopeControls = document.querySelector<HTMLElement>(
        '[data-slot="data-page-controls"]',
      );
      const toolbarContent = toolbar;
      const empty = document.querySelector<HTMLElement>(
        '[data-slot="connections-no-connections"]',
      );
      const closeButton = toolbar?.querySelector<HTMLButtonElement>(
        '[data-slot="connections-close-all"]',
      );
      const search = toolbar?.querySelector<HTMLInputElement>(
        '[data-slot="connections-search"]',
      );
      const rect = (element: HTMLElement | null | undefined) =>
        element
          ? {
              x: Math.round(element.getBoundingClientRect().x),
              y: Math.round(element.getBoundingClientRect().y),
              width: Math.round(element.getBoundingClientRect().width),
              height: Math.round(element.getBoundingClientRect().height),
            }
          : null;

      const toolbarStyle = toolbarContent
        ? getComputedStyle(toolbarContent)
        : null;
      const searchField = search?.closest<HTMLElement>(
        '[data-slot="search-field"]',
      );
      const searchStyle = searchField ? getComputedStyle(searchField) : null;
      const closeButtonStyle = closeButton
        ? getComputedStyle(closeButton)
        : null;

      return {
        layout: rect(layout),
        page: rect(page),
        surface: rect(surface),
        dock: rect(dock),
        actions: rect(actions),
        searchField: rect(searchField),
        scopeControls: rect(scopeControls),
        container: rect(container),
        scroll: rect(scroll),
        toolbar: rect(toolbar),
        toolbarContent: rect(toolbarContent),
        empty: rect(empty),
        closeButton: rect(closeButton),
        closeButtonDisabled: closeButton?.disabled ?? null,
        search: rect(search),
        emptyText: empty?.innerText ?? '',
        toolbarDisplay: toolbarStyle?.display ?? '',
        toolbarGap: toolbarStyle?.gap ?? '',
        searchBorderRadius: searchStyle?.borderRadius ?? '',
        closeButtonBorderRadius: closeButtonStyle?.borderRadius ?? '',
        viewport: { width: window.innerWidth, height: window.innerHeight },
      };
    });

    // X11 window decoration reduces the WebKit content viewport below the
    // requested outer 1240px. The reference dock still uses its real bounds.
    assert.ok(state.viewport.width >= 1100, JSON.stringify(state, null, 2));
    assert.ok(state.layout, JSON.stringify(state, null, 2));
    assert.ok(state.container, JSON.stringify(state, null, 2));
    assert.ok(state.page, JSON.stringify(state, null, 2));
    assert.ok(state.surface, JSON.stringify(state, null, 2));
    assert.ok(state.dock, JSON.stringify(state, null, 2));
    assert.ok(state.toolbar, JSON.stringify(state, null, 2));
    assert.ok(state.actions, JSON.stringify(state, null, 2));
    assert.ok(state.scopeControls, JSON.stringify(state, null, 2));
    // ref floating DataPage toolbar is docked above the page bottom, not
    // inserted into the scrollable table or laid out above it.
    assert.ok(
      state.dock.y >= state.page.y + state.page.height - 140,
      JSON.stringify(state, null, 2),
    );
    assert.ok(
      state.dock.y + state.dock.height <= state.page.y + state.page.height,
      JSON.stringify(state, null, 2),
    );
    assert.ok(state.toolbar.y >= state.dock.y, JSON.stringify(state, null, 2));
    assert.ok(
      (state.scroll?.height ?? 0) > 400,
      JSON.stringify(state, null, 2),
    );
    assert.ok(
      (state.search?.width ?? 0) >= 190,
      JSON.stringify(state, null, 2),
    );
    assert.ok(
      (state.searchField?.height ?? 0) >= 40,
      JSON.stringify(state, null, 2),
    );
    assert.ok(
      (state.closeButton?.width ?? 0) >= 32,
      JSON.stringify(state, null, 2),
    );
    assert.equal(
      state.closeButtonDisabled,
      false,
      JSON.stringify(state, null, 2),
    );
    assert.ok(state.emptyText.length > 0, JSON.stringify(state, null, 2));
    assert.equal(state.toolbarDisplay, 'flex', JSON.stringify(state, null, 2));
    assert.ok(
      Number.parseFloat(state.toolbarGap) >= 8,
      JSON.stringify(state, null, 2),
    );
    assert.ok(
      Number.parseFloat(state.searchBorderRadius) > 10_000,
      JSON.stringify(state, null, 2),
    );
    assert.ok(
      Number.parseFloat(state.closeButtonBorderRadius) > 10_000,
      JSON.stringify(state, null, 2),
    );

    const evidencePath = process.env.CHIMERA_E2E_EVIDENCE_PATH;
    if (evidencePath) {
      fs.mkdirSync(path.dirname(evidencePath), { recursive: true });
      await browser.saveScreenshot(evidencePath);
    }
  });

  it('uses the ref glass SearchField clear action and synchronizes the route', async () => {
    const search = await displayedElement('[data-slot="connections-search"]');
    await focusElement(search);
    await browser.keys('__chimera_ref_search_clear__');

    const clearButton = await displayedElement(
      '[data-slot="data-page-dock"] [data-slot="search-field-clear"]',
    );
    await clearButton.waitForClickable({ timeout: 10_000 });
    await browser.waitUntil(
      async () => new URL(await browser.getUrl()).searchParams.has('q'),
      {
        timeout: 10_000,
        timeoutMsg: 'Connections search did not synchronize to the router.',
      },
    );

    await clearButton.click();
    assert.equal(await search.getValue(), '');
    await browser.waitUntil(
      async () => !new URL(await browser.getUrl()).searchParams.has('q'),
      {
        timeout: 10_000,
        timeoutMsg: 'Clearing the reference search did not remove q from URL.',
      },
    );
  });

  /* Contract CONN-URL-SCOPE: selecting a scope tab updates router state. */
  it('keeps the selected scope in the route', async () => {
    const closedTab = await displayedElement(
      '[data-slot="connections-scope-tab"][data-scope="closed"]',
    );
    await closedTab.click();
    await browser.waitUntil(
      async () =>
        browser.execute(
          () =>
            document
              .querySelector(
                '[data-slot="connections-scope-tab"][data-scope="closed"]',
              )
              ?.getAttribute('data-state') === 'on' &&
            location.search.includes('scope=closed'),
        ),
      {
        timeout: 10_000,
        timeoutMsg: 'Closed scope was not written to the route.',
      },
    );

    const activeTab = await displayedElement(
      '[data-slot="connections-scope-tab"][data-scope="active"]',
    );
    await activeTab.click();
    await browser.waitUntil(
      async () =>
        browser.execute(
          () =>
            document
              .querySelector(
                '[data-slot="connections-scope-tab"][data-scope="active"]',
              )
              ?.getAttribute('data-state') === 'on' &&
            location.search.includes('scope=active'),
        ),
      {
        timeout: 10_000,
        timeoutMsg: 'Active scope was not written to the route.',
      },
    );
  });

  it('opens the reference column-settings dialog from the floating actions', async () => {
    const button = await displayedElement(
      '[data-slot="data-page-dock"] [data-slot="connections-column-settings"]',
    );
    await button.waitForClickable({ timeout: 15_000 });
    await button.click();
    const first = await displayedElement(
      '[data-slot="connections-column-settings-item"]:first-child',
    );
    assert.ok(await first.isDisplayed());

    const state = await browser.execute(() => ({
      dialog: Boolean(document.querySelector('[data-slot="modal-content"]')),
      columns: document.querySelectorAll(
        '[data-slot="connections-column-settings-item"]',
      ).length,
    }));
    assert.equal(state.dialog, true, JSON.stringify(state));
    assert.ok(state.columns >= 8, JSON.stringify(state));
  });
});
