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
        '[data-slot="connections-container"]',
      );
      const scroll = document.querySelector<HTMLElement>(
        '[data-slot="connections-scroll-wrapper"]',
      );
      const toolbar = document.querySelector<HTMLElement>(
        '[data-slot="connections-toolbar"]',
      );
      const toolbarContent = document.querySelector<HTMLElement>(
        '[data-slot="connections-toolbar-content"]',
      );
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
      const searchStyle = search ? getComputedStyle(search) : null;
      const closeButtonStyle = closeButton
        ? getComputedStyle(closeButton)
        : null;

      return {
        layout: rect(layout),
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
        toolbarPaddingInline: toolbarStyle?.paddingInline ?? '',
        searchBorderRadius: searchStyle?.borderRadius ?? '',
        closeButtonBorderRadius: closeButtonStyle?.borderRadius ?? '',
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
    assert.equal(
      state.closeButtonDisabled,
      false,
      JSON.stringify(state, null, 2),
    );
    assert.ok(state.emptyText.length > 0, JSON.stringify(state, null, 2));
    assert.equal(state.toolbarDisplay, 'flex', JSON.stringify(state, null, 2));
    assert.equal(state.toolbarGap, '12px', JSON.stringify(state, null, 2));
    assert.equal(
      state.toolbarPaddingInline,
      '16px',
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
});
