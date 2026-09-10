import assert from 'node:assert/strict';
import fs from 'node:fs';
import path from 'node:path';
import { openMainRoute } from './main-window.js';

describe('main ref dropdown menu', () => {
  it('uses the ref menu geometry without changing the legacy primitive', async () => {
    await browser.setWindowSize(1240, 638);
    await openMainRoute('/main/dashboard');
    await browser.setWindowSize(1240, 638);

    const appHeader = await $('[data-slot="app-header"]');
    const settingsButton = await appHeader.$(
      '[data-slot="header-settings-menu"]',
    );
    await settingsButton.waitForDisplayed({ timeout: 15_000 });
    await browser.execute((button) => button.focus(), settingsButton);
    await browser.keys('Enter');

    const openState = await browser.execute(() => {
      const trigger = document.querySelector<HTMLButtonElement>(
        '[data-slot="header-settings-menu"]',
      );
      return {
        triggerState: trigger?.getAttribute('data-state') ?? '',
        roleMenuCount: document.querySelectorAll('[role="menu"]').length,
        bodyText: document.body.innerText,
      };
    });
    console.log('main dropdown open state', openState);

    assert.equal(
      openState.triggerState,
      'open',
      JSON.stringify(openState, null, 2),
    );

    const content = await $('[data-slot="main-dropdown-menu-motion-content"]');
    await content.waitForDisplayed({ timeout: 15_000 });
    await browser.waitUntil(
      async () =>
        browser.execute(() => {
          const menu = document.querySelector<HTMLElement>(
            '[data-slot="main-dropdown-menu-motion-content"]',
          );
          const transform = menu ? getComputedStyle(menu).transform : '';
          return (
            transform === 'none' || transform === 'matrix(1, 0, 0, 1, 0, 0)'
          );
        }),
      { timeout: 15_000, timeoutMsg: 'Dropdown animation did not settle.' },
    );

    const evidencePath = process.env.CHIMERA_E2E_EVIDENCE_PATH;
    if (evidencePath) {
      fs.mkdirSync(path.dirname(evidencePath), { recursive: true });
      await browser.saveScreenshot(evidencePath);
    }

    const state = await browser.execute(() => {
      const menu = document.querySelector<HTMLElement>(
        '[data-slot="main-dropdown-menu-motion-content"]',
      );
      const items = menu ? Array.from(menu.children) : [];
      const style = menu ? getComputedStyle(menu) : null;

      return {
        borderRadius: style?.borderRadius ?? '',
        backgroundColor: style?.backgroundColor ?? '',
        itemHeights: items.map((item) =>
          Math.round(Number.parseFloat(getComputedStyle(item).height)),
        ),
        viewport: { width: innerWidth, height: innerHeight },
      };
    });

    assert.ok(state.viewport.width >= 1200, JSON.stringify(state, null, 2));
    assert.ok(state.viewport.height >= 600, JSON.stringify(state, null, 2));
    assert.equal(state.borderRadius, '4px', JSON.stringify(state, null, 2));
    assert.notEqual(
      state.backgroundColor,
      'rgba(0, 0, 0, 0)',
      JSON.stringify(state, null, 2),
    );
    assert.deepEqual(
      state.itemHeights,
      [48, 48, 48],
      JSON.stringify(state, null, 2),
    );
  });
});
