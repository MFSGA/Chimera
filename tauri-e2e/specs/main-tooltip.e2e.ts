import assert from 'node:assert/strict';
import fs from 'node:fs';
import path from 'node:path';
import { openMainRoute } from './main-window.js';

const targetPath = '/main/logs';

describe('main tooltip reference surface', () => {
  before(async () => {
    await browser.setWindowSize(1240, 638);
    await browser.execute(() => {
      localStorage.setItem(btoa('paraglide-language-cache'), 'zh-cn');
    });
    await openMainRoute(targetPath);
    await browser.setWindowSize(1240, 638);
  });

  it('keeps ref padding on the animated inner surface', async () => {
    const firstSidebarLink = await $('[data-slot="slider-sidebar"] a');
    await firstSidebarLink.waitForDisplayed({ timeout: 15_000 });
    await browser.execute((element) => {
      element.dispatchEvent(
        new PointerEvent('pointermove', {
          bubbles: true,
          pointerType: 'mouse',
        }),
      );
    }, firstSidebarLink);

    await browser.waitUntil(
      async () =>
        browser.execute(() =>
          Boolean(document.querySelector<HTMLElement>('[role="tooltip"]')),
        ),
      { timeout: 15_000, timeoutMsg: 'Tooltip did not open.' },
    );

    const state = await browser.execute(() => {
      const tooltip = document.querySelector<HTMLElement>('[role="tooltip"]');
      const surface = tooltip?.firstElementChild as HTMLElement | null;
      const content = surface?.firstElementChild as HTMLElement | null;

      const style = (element: HTMLElement | null) =>
        element ? getComputedStyle(element) : null;
      const tooltipStyle = style(tooltip);
      const surfaceStyle = style(surface);

      return {
        tooltipChildren: tooltip?.children.length ?? 0,
        surfaceChildren: surface?.children.length ?? 0,
        tooltipPadding: tooltipStyle
          ? [
              tooltipStyle.paddingTop,
              tooltipStyle.paddingRight,
              tooltipStyle.paddingBottom,
              tooltipStyle.paddingLeft,
            ]
          : [],
        surfacePadding: surfaceStyle
          ? [
              surfaceStyle.paddingTop,
              surfaceStyle.paddingRight,
              surfaceStyle.paddingBottom,
              surfaceStyle.paddingLeft,
            ]
          : [],
        surfaceOverflow: surfaceStyle?.overflow ?? '',
        contentText: content?.textContent?.trim() ?? '',
        borderRadius: tooltipStyle?.borderRadius ?? '',
        viewport: { width: innerWidth, height: innerHeight },
      };
    });

    assert.deepEqual(state.tooltipPadding, ['0px', '0px', '0px', '0px']);
    assert.deepEqual(state.surfacePadding, ['6px', '12px', '6px', '12px']);
    assert.equal(state.surfaceOverflow, 'hidden');
    assert.equal(state.tooltipChildren, 1, JSON.stringify(state, null, 2));
    assert.equal(state.surfaceChildren, 1, JSON.stringify(state, null, 2));
    assert.ok(state.contentText.length > 0, JSON.stringify(state, null, 2));
    assert.notEqual(state.borderRadius, '0px', JSON.stringify(state, null, 2));
    assert.equal(state.viewport.width, 1224);
    assert.equal(state.viewport.height, 629);

    const evidencePath = process.env.CHIMERA_E2E_EVIDENCE_PATH;
    if (evidencePath) {
      fs.mkdirSync(path.dirname(evidencePath), { recursive: true });
      await browser.saveScreenshot(evidencePath);
    }
  });
});
