import assert from 'node:assert/strict';
import fs from 'node:fs';
import path from 'node:path';
import { openMainRoute } from './main-window.js';

describe('main about settings reference layout', () => {
  before(async () => {
    await browser.setWindowSize(1240, 638);
    await browser.execute(() => {
      localStorage.setItem(btoa('paraglide-language-cache'), 'zh-cn');
    });
    await openMainRoute('/main/settings/about');
    await browser.setWindowSize(1240, 638);
    // A previous settings route can remain mounted while its exit animates.
    // Wait for the actual About outlet, not merely the updated URL.
    await browser.waitUntil(
      async () =>
        browser.execute(() => {
          const active = document.querySelector<HTMLElement>(
            '[data-route-id="/(main)/main/settings/about"][data-route-present="true"]',
          );
          return Boolean(
            active && Number(getComputedStyle(active).opacity) >= 0.99,
          );
        }),
      {
        timeout: 15_000,
        timeoutMsg: 'The active About settings route did not finish entering.',
      },
    );
  });

  it('matches the ref title, grid, and version-card CSS contract', async () => {
    // The saved Windows failure screenshot showed the About card clearly
    // rendered, while the embedded WebDriver's isDisplayed still returned
    // false. Assert actual layout visibility inside the active route instead
    // of trusting the driver's stale element visibility result.
    await browser.waitUntil(
      async () =>
        browser.execute(() => {
          const activeRoute = document.querySelector<HTMLElement>(
            '[data-route-id="/(main)/main/settings/about"][data-route-present="true"]',
          );
          const card = activeRoute?.querySelector<HTMLElement>(
            '[data-slot="settings-card"]',
          );
          if (!card) return false;
          const rect = card.getBoundingClientRect();
          const style = getComputedStyle(card);
          return (
            rect.width > 0 &&
            rect.height > 0 &&
            style.display !== 'none' &&
            style.visibility !== 'hidden'
          );
        }),
      {
        timeout: 15_000,
        timeoutMsg: 'The active About settings card has no visible layout.',
      },
    );

    const state = await browser.execute(() => {
      const activeRoute = document.querySelector<HTMLElement>(
        '[data-route-id="/(main)/main/settings/about"][data-route-present="true"]',
      );
      const titleSlots = Array.from(
        activeRoute?.querySelectorAll<HTMLElement>(
          '[data-slot="settings-title"]',
        ) ?? [],
      );
      const stickyTitle = titleSlots[0];
      const mainTitle = titleSlots[1];
      const visibleTitle = mainTitle?.querySelector<HTMLElement>('p');
      const versionCard = Array.from(
        activeRoute?.querySelectorAll<HTMLElement>(
          '[data-slot="settings-card"]',
        ) ?? [],
      ).find((candidate) => {
        const parent = candidate.parentElement;
        return parent && getComputedStyle(parent).display === 'grid';
      });
      const grid = versionCard?.parentElement;
      const pageContent = grid?.parentElement;

      if (
        !stickyTitle ||
        !mainTitle ||
        !visibleTitle ||
        !versionCard ||
        !grid ||
        !pageContent
      ) {
        throw new Error('About settings reference DOM is incomplete.');
      }

      const stickyStyle = getComputedStyle(stickyTitle);
      const mainStyle = getComputedStyle(mainTitle);
      const titleStyle = getComputedStyle(visibleTitle);
      const gridStyle = getComputedStyle(grid);
      const contentStyle = getComputedStyle(pageContent);
      const cardStyle = getComputedStyle(versionCard);

      return {
        path: location.pathname,
        viewport: { width: innerWidth, height: innerHeight },
        sticky: {
          height: stickyStyle.height,
          paddingLeft: stickyStyle.paddingLeft,
          paddingRight: stickyStyle.paddingRight,
          position: stickyStyle.position,
          top: stickyStyle.top,
          zIndex: stickyStyle.zIndex,
        },
        mainTitle: {
          height: mainStyle.height,
          paddingLeft: mainStyle.paddingLeft,
          paddingTop: mainStyle.paddingTop,
          paddingBottom: mainStyle.paddingBottom,
        },
        title: {
          fontSize: titleStyle.fontSize,
          fontWeight: titleStyle.fontWeight,
          lineHeight: titleStyle.lineHeight,
        },
        content: {
          paddingLeft: contentStyle.paddingLeft,
          paddingRight: contentStyle.paddingRight,
          paddingBottom: contentStyle.paddingBottom,
        },
        grid: {
          display: gridStyle.display,
          gap: gridStyle.gap,
          columns: gridStyle.gridTemplateColumns,
        },
        card: {
          borderRadius: cardStyle.borderRadius,
          width: cardStyle.width,
        },
      };
    });

    assert.equal(state.path, '/main/settings/about');
    assert.ok(state.viewport.width >= 1200, JSON.stringify(state, null, 2));
    assert.ok(state.viewport.height >= 600, JSON.stringify(state, null, 2));
    assert.deepEqual(state.sticky, {
      height: '64px',
      paddingLeft: '24px',
      paddingRight: '24px',
      position: 'sticky',
      top: '0px',
      zIndex: '10',
    });
    assert.deepEqual(state.mainTitle, {
      height: '96px',
      paddingLeft: '24px',
      paddingTop: '40px',
      paddingBottom: '16px',
    });
    assert.deepEqual(state.title, {
      fontSize: '30px',
      fontWeight: '700',
      lineHeight: '36px',
    });
    assert.deepEqual(state.content, {
      paddingLeft: '16px',
      paddingRight: '16px',
      paddingBottom: '16px',
    });
    assert.equal(state.grid.display, 'grid');
    assert.equal(state.grid.gap, '8px');
    assert.equal(state.grid.columns.split(' ').length, 2);
    assert.match(state.card.borderRadius, /^\d+(?:\.\d+)?px$/);

    const evidencePath = process.env.CHIMERA_E2E_EVIDENCE_PATH;
    if (evidencePath) {
      fs.mkdirSync(path.dirname(evidencePath), { recursive: true });
      await browser.saveScreenshot(evidencePath);
    }
  });
});
