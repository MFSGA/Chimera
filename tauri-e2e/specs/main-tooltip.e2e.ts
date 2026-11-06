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
    // A WebDriver .moveTo() from the previous pointer position can sweep
    // through the native Windows Settings menu. The failed desktop screenshot
    // showed that menu open, occluding subsequent hover. Dismiss it via a real
    // keyboard action and move the pointer to a safe content point first.
    await browser.keys('Escape');
    // Observe the delivered event kind; Radix opens hover tooltips only for
    // mouse pointer events, not generic/pen/touch pointer motion.
    await browser.execute((link) => {
      link.dataset.e2ePointerType = 'none';
      link.dataset.e2ePointerTrusted = 'none';
      link.dataset.e2ePointerMoves = '0';
      link.addEventListener('pointermove', (event) => {
        link.dataset.e2ePointerType = event.pointerType;
        link.dataset.e2ePointerTrusted = String(event.isTrusted);
        link.dataset.e2ePointerMoves = String(
          Number(link.dataset.e2ePointerMoves ?? 0) + 1,
        );
      });
    }, firstSidebarLink);
    await browser
      .action('pointer', { parameters: { pointerType: 'mouse' } })
      .move({ x: 1100, y: 420, duration: 0 })
      .perform();
    await browser
      .action('pointer', { parameters: { pointerType: 'mouse' } })
      .move({ origin: firstSidebarLink, duration: 0 })
      .perform();

    try {
      await browser.waitUntil(
        async () =>
          browser.execute(() =>
            Boolean(document.querySelector<HTMLElement>('[role="tooltip"]')),
          ),
        { timeout: 15_000, timeoutMsg: 'Tooltip did not open.' },
      );
    } catch (cause) {
      const observed = await browser.execute((link) => {
        const rect = link.getBoundingClientRect();
        const x = Math.round(rect.left + rect.width / 2);
        const y = Math.round(rect.top + rect.height / 2);
        const top = document.elementFromPoint(x, y);
        return {
          center: { x, y },
          topTag: top?.tagName ?? null,
          centerHitsTrigger: top === link || link.contains(top),
          triggerState: link.getAttribute('data-state'),
          pointerType: link.dataset.e2ePointerType ?? null,
          pointerTrusted: link.dataset.e2ePointerTrusted ?? null,
          pointerMoves: link.dataset.e2ePointerMoves ?? null,
          tooltipCount: document.querySelectorAll('[role="tooltip"]').length,
          viewport: { width: innerWidth, height: innerHeight },
        };
      }, firstSidebarLink);
      throw new Error(
        'Real WebDriver tooltip hover failed: ' + JSON.stringify(observed),
        {
          cause,
        },
      );
    }

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
    assert.ok(state.viewport.width >= 1200, JSON.stringify(state, null, 2));
    assert.ok(state.viewport.height >= 600, JSON.stringify(state, null, 2));

    const evidencePath = process.env.CHIMERA_E2E_EVIDENCE_PATH;
    if (evidencePath) {
      fs.mkdirSync(path.dirname(evidencePath), { recursive: true });
      await browser.saveScreenshot(evidencePath);
    }
  });
});
