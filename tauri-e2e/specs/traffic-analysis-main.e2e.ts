import assert from 'node:assert/strict';
import { displayedElement, focusElement } from './interaction.js';
import { ensureMainWindow } from './main-window.js';

const sidebarToggleSelector = '[data-slot="navigation-sidebar-toggle"]';

async function readSidebarToggleState() {
  return browser.execute((selector) => {
    const toggle = document.querySelector(selector);
    return toggle
      ? {
          expanded: toggle.getAttribute('aria-expanded'),
          label: toggle.getAttribute('aria-label'),
        }
      : null;
  }, sidebarToggleSelector);
}

type SidebarToggleState = Awaited<ReturnType<typeof readSidebarToggleState>>;

async function waitForSidebarToggleState(
  matches: (state: SidebarToggleState) => boolean,
  timeoutMessage: string,
) {
  let lastState: SidebarToggleState = null;

  try {
    await browser.waitUntil(
      async () => {
        lastState = await readSidebarToggleState();
        return matches(lastState);
      },
      { timeout: 10_000, timeoutMsg: timeoutMessage },
    );
  } catch (error) {
    try {
      lastState = await readSidebarToggleState();
    } catch (diagnosticError) {
      throw new AggregateError(
        [error, diagnosticError],
        `${timeoutMessage} The final sidebar state could not be read. Last observed state: ${JSON.stringify(lastState)}.`,
        { cause: error },
      );
    }

    throw new Error(
      `${timeoutMessage} Last observed state: ${JSON.stringify(lastState)}.`,
      { cause: error },
    );
  }
}

describe('main traffic analysis', () => {
  beforeEach(async () => {
    await ensureMainWindow();
    await browser.setWindowSize(1280, 720);

    const currentPath = await browser.execute(() => location.pathname);
    if (currentPath !== '/main/dashboard') {
      const dashboardLink = await $(
        '[data-slot="app-navbar"] a[href="/main/dashboard"]',
      );
      await dashboardLink.waitForDisplayed({ timeout: 10_000 });
      await dashboardLink.click();
    }

    await browser.waitUntil(
      async () =>
        browser.execute(
          () =>
            location.pathname === '/main/dashboard' &&
            document.querySelector('[data-slot="dashboard-container"]') !==
              null,
        ),
      {
        timeout: 15_000,
        timeoutMsg: 'The dashboard did not open before traffic navigation.',
      },
    );
  });

  /* Contract TRAFFIC-URL-STATE: visible controls own the route's traffic query. */
  it('opens from main navigation and carries scope and metric between analysis views', async () => {
    const trafficLink = await $(
      '[data-slot="app-navbar"] a[href="/main/topology"]',
    );
    await trafficLink.waitForDisplayed({ timeout: 10_000 });
    await trafficLink.click();

    await browser.waitUntil(
      async () =>
        browser.execute(
          () =>
            location.pathname === '/main/topology' &&
            document.querySelector('[data-slot="traffic-page"]') !== null,
        ),
      {
        timeout: 15_000,
        timeoutMsg: 'The traffic overview did not open from main navigation.',
      },
    );

    const sidebarToggle = await $(sidebarToggleSelector);
    const collapsedState = await readSidebarToggleState();
    const collapsedLabel = collapsedState?.label;
    assert.ok(collapsedLabel);
    assert.equal(collapsedState?.expanded, 'false');
    await sidebarToggle.click();
    await waitForSidebarToggleState(
      (state) =>
        state?.expanded === 'true' &&
        Boolean(state.label) &&
        state.label !== collapsedLabel,
      'Expanding the traffic sidebar did not update its label.',
    );
    const expandedLabel = (await readSidebarToggleState())?.label;
    assert.ok(expandedLabel);
    assert.notEqual(expandedLabel, collapsedLabel);
    await $(sidebarToggleSelector).click();
    await waitForSidebarToggleState(
      (state) => state?.expanded === 'false' && state.label === collapsedLabel,
      'Collapsing the traffic sidebar did not restore its label.',
    );

    const closedScopeSelector =
      '[data-slot="traffic-scope-item"][data-scope="closed"]';
    await $(closedScopeSelector).click();
    await browser.waitUntil(
      async () =>
        browser.execute(
          () => new URLSearchParams(location.search).get('scope') === 'closed',
        ),
      {
        timeout: 10_000,
        timeoutMsg: 'Selecting Closed did not update the traffic URL.',
      },
    );
    await browser.waitUntil(
      async () =>
        browser.execute(
          (selector) =>
            document.querySelector(selector)?.getAttribute('aria-checked') ===
            'true',
          closedScopeSelector,
        ),
      {
        timeout: 10_000,
        timeoutMsg: 'The Closed scope control did not settle after navigation.',
      },
    );

    const metricTrigger = await displayedElement(
      '[data-slot="traffic-metric-trigger"]',
    );
    await focusElement(metricTrigger);
    await browser.keys('Enter');
    await browser.waitUntil(
      async () =>
        (await metricTrigger.getAttribute('aria-expanded')) === 'true',
      {
        timeout: 10_000,
        timeoutMsg: 'The traffic metric menu did not open after activation.',
      },
    );
    await browser.keys('ArrowDown');
    await browser.keys('Enter');
    await browser.waitUntil(
      async () =>
        browser.execute(
          () =>
            new URLSearchParams(location.search).get('metric') ===
            'connections',
        ),
      {
        timeout: 10_000,
        timeoutMsg: 'Selecting Connections did not update the traffic URL.',
      },
    );

    const flowLink = await $(
      '[data-slot="traffic-navigation-item"][href^="/main/topology/flow"]',
    );
    await flowLink.click();
    await browser.waitUntil(
      async () =>
        browser.execute(() => {
          const search = new URLSearchParams(location.search);
          return (
            location.pathname === '/main/topology/flow' &&
            search.get('scope') === 'closed' &&
            search.get('metric') === 'connections' &&
            document.querySelector(
              '[data-slot="traffic-navigation-item"][href^="/main/topology/flow"][data-active="true"]',
            ) !== null
          );
        }),
      {
        timeout: 15_000,
        timeoutMsg: 'The flow route lost the selected traffic query state.',
      },
    );

    const mapLink = await $(
      '[data-slot="traffic-navigation-item"][href^="/main/topology/map"]',
    );
    await mapLink.click();
    await browser.waitUntil(
      async () =>
        browser.execute(() => {
          const search = new URLSearchParams(location.search);
          return (
            location.pathname === '/main/topology/map' &&
            search.get('scope') === 'closed' &&
            search.get('metric') === 'connections' &&
            document.querySelector(
              '[data-slot="traffic-navigation-item"][href^="/main/topology/map"][data-active="true"]',
            ) !== null
          );
        }),
      {
        timeout: 15_000,
        timeoutMsg: 'The map route lost the selected traffic query state.',
      },
    );

    const state = await browser.execute(() => ({
      pathname: location.pathname,
      search: location.search,
      page: document.querySelector('[data-slot="traffic-page"]') !== null,
      map:
        document.querySelector(
          '[data-slot="traffic-navigation-item"][href^="/main/topology/map"][data-active="true"]',
        ) !== null,
    }));
    assert.deepEqual(state, {
      pathname: '/main/topology/map',
      search: state.search,
      page: true,
      map: true,
    });
    assert.equal(new URLSearchParams(state.search).get('scope'), 'closed');
    assert.equal(
      new URLSearchParams(state.search).get('metric'),
      'connections',
    );
  });
});
