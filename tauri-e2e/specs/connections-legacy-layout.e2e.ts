import assert from 'node:assert/strict';
import { displayedElement, focusElement } from './interaction.js';

/*
 * Contract CONN-LEGACY-SHARED (desktop UI E2E; smoke suite):
 * Start in the harness's isolated legacy window and open its Connections route.
 * Search for an impossible connection, then select Closed through real controls.
 * The page must show the same Connections data slots and selectable scopes as
 * the main route, with an independent usable table viewport and fixed toolbar.
 * A legacy-only table, nested non-scrolling viewport, or disconnected tabs
 * makes an assertion fail. No profiles, connections, or system settings mutate.
 * This is not proof of real network traffic, native pointer events, or an
 * exact pixel-for-pixel reference screenshot match.
 */
describe('legacy connections shared layout', () => {
  beforeEach(async () => {
    await browser.switchToWindow('legacy');
    await browser.setWindowSize(1240, 638);
    const focusState = await browser.execute(async () => {
      const internals = (
        window as typeof window & {
          __TAURI_INTERNALS__: {
            invoke: (
              command: string,
              args?: Record<string, unknown>,
            ) => Promise<void>;
          };
        }
      ).__TAURI_INTERNALS__;
      try {
        await internals.invoke('plugin:window|set_focus', { label: 'legacy' });
        return { focused: true, error: null };
      } catch (error) {
        return { focused: false, error: String(error) };
      }
    });
    console.log('Legacy window focus state:', JSON.stringify(focusState));

    // Enter through real legacy sidebar controls, avoiding a full webview
    // reload that can leave the legacy page-transition opacity at zero.
    const dashboard = await displayedElement(
      '[data-testid="sidebar-route-dashboard"]',
    );
    await dashboard.click();
    const connections = await displayedElement(
      '[data-testid="sidebar-route-connections"]',
    );
    await connections.click();
    await browser.waitUntil(
      async () => browser.execute(() => location.pathname === '/connections'),
      {
        timeout: 10_000,
        timeoutMsg: 'Legacy sidebar did not navigate to Connections.',
      },
    );

    try {
      await displayedElement('[data-slot="connections-search"]');
    } catch (cause) {
      const diagnostics = await browser.execute(() => {
        const element = document.querySelector<HTMLElement>(
          '[data-slot="connections-search"]',
        );
        const parents: unknown[] = [];
        let current: HTMLElement | null = element;
        while (current && parents.length < 12) {
          const rect = current.getBoundingClientRect();
          const style = getComputedStyle(current);
          parents.push({
            tag: current.tagName,
            className: current.className,
            width: rect.width,
            height: rect.height,
            display: style.display,
            visibility: style.visibility,
            opacity: style.opacity,
            overflow: style.overflow,
          });
          current = current.parentElement;
        }
        return {
          href: location.href,
          visibility: document.visibilityState,
          hasFocus: document.hasFocus(),
          reducedMotion: matchMedia('(prefers-reduced-motion: reduce)').matches,
          animations: document
            .getAnimations()
            .slice(0, 8)
            .map((animation) => ({
              state: animation.playState,
              currentTime: animation.currentTime,
            })),
          parents,
        };
      });
      throw new Error(
        `Legacy Connections invisible: ${JSON.stringify(diagnostics)}`,
        { cause },
      );
    }
  });

  it('renders the shared table viewport below the legacy header', async () => {
    const layout = await browser.execute(() => {
      const getRect = (selector: string) => {
        const element = document.querySelector<HTMLElement>(selector);
        if (!element) return null;
        const rect = element.getBoundingClientRect();
        return {
          top: rect.top,
          bottom: rect.bottom,
          height: rect.height,
          width: rect.width,
        };
      };
      const viewport = document.querySelector<HTMLElement>(
        '[data-slot="connections-scroll-wrapper"] [data-slot="scroll-area-viewport"]',
      );
      return {
        path: location.pathname,
        windowHeight: innerHeight,
        header: getRect('.MDYBasePage > header'),
        container: getRect('[data-slot="connections-container"]'),
        toolbar: getRect('[data-slot="connections-toolbar"]'),
        viewport: getRect(
          '[data-slot="connections-scroll-wrapper"] [data-slot="scroll-area-viewport"]',
        ),
        viewportOverflow: viewport
          ? getComputedStyle(viewport).overflowY
          : null,
        mainOnlyRoot: !!document.querySelector('[data-slot="app-root"]'),
      };
    });

    assert.equal(layout.path, '/connections');
    assert.equal(layout.mainOnlyRoot, false);
    assert.ok(layout.header, JSON.stringify(layout));
    assert.ok(layout.container, JSON.stringify(layout));
    assert.ok(layout.toolbar, JSON.stringify(layout));
    assert.ok(layout.viewport, JSON.stringify(layout));
    assert.ok((layout.viewport?.height ?? 0) > 180, JSON.stringify(layout));
    assert.ok(
      (layout.viewport?.top ?? 0) >= (layout.header?.bottom ?? 0),
      JSON.stringify(layout),
    );
    assert.ok(
      (layout.toolbar?.top ?? 0) >= (layout.viewport?.bottom ?? 0) - 3,
      JSON.stringify(layout),
    );
    assert.ok(
      (layout.toolbar?.bottom ?? Infinity) <= layout.windowHeight + 3,
      JSON.stringify(layout),
    );
    assert.notEqual(layout.viewportOverflow, 'hidden');
  });

  it('opens the reference column settings from the legacy toolbar', async () => {
    const settings = await displayedElement(
      '[data-slot="connections-column-settings"]',
    );
    await settings.click();
    const item = await displayedElement(
      '[data-slot="connections-column-settings-item"]:first-child',
    );
    assert.ok(await item.isDisplayed());
    const state = await browser.execute(() => ({
      items: document.querySelectorAll(
        '[data-slot="connections-column-settings-item"]',
      ).length,
      dialog: Boolean(document.querySelector('[data-slot="modal-content"]')),
    }));
    assert.ok(state.dialog, JSON.stringify(state));
    assert.ok(state.items >= 8, JSON.stringify(state));
  });

  it('filters through the shared search and switches connection scope', async () => {
    const search = await displayedElement('[data-slot="connections-search"]');
    await focusElement(search);
    await browser.keys('__chimera_legacy_no_matching_connection__');

    await browser.waitUntil(
      async () =>
        browser.execute(() => {
          const element = document.querySelector<HTMLElement>(
            '[data-slot="connections-no-connections"]',
          );
          return Boolean(element?.innerText.trim());
        }),
      {
        timeout: 15_000,
        timeoutMsg: 'Legacy Connections search did not show an empty state.',
      },
    );

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
              ?.getAttribute('data-state') === 'on',
        ),
      { timeout: 10_000, timeoutMsg: 'Legacy Closed scope did not select.' },
    );

    assert.equal(
      await search.getValue(),
      '__chimera_legacy_no_matching_connection__',
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
              ?.getAttribute('data-state') === 'on',
        ),
      { timeout: 10_000, timeoutMsg: 'Legacy Active scope did not restore.' },
    );
  });
});
