import assert from 'node:assert/strict';
import { openMainRoute } from './main-window.js';

describe('main traffic analysis', () => {
  before(async () => {
    await browser.setWindowSize(1280, 720);
    await browser.execute(() => {
      localStorage.setItem(btoa('paraglide-language-cache'), 'en');
    });
  });

  beforeEach(async () => {
    await openMainRoute('/main/dashboard');
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

    const sidebarToggle = await $('[data-slot="navigation-sidebar-toggle"]');
    assert.ok(await sidebarToggle.getAttribute('aria-label'));

    const closedScope = await $(
      '[data-slot="traffic-scope-item"][data-scope="closed"]',
    );
    await closedScope.click();
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

    const metricTrigger = await $('[data-slot="traffic-metric-trigger"]');
    await metricTrigger.click();
    const connectionsMetric = await $(
      '[data-slot="traffic-metric-option"][data-value="connections"]',
    );
    await connectionsMetric.waitForDisplayed({ timeout: 5_000 });
    await connectionsMetric.click();
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
