import assert from 'node:assert/strict';
import fs from 'node:fs';
import path from 'node:path';
import { openMainRoute } from './main-window.js';

const targetPath = '/main/rules';

describe('main tooltip reference surface', () => {
  before(async () => {
    await browser.setWindowSize(1240, 638);
    await browser.execute(() => {
      localStorage.setItem(btoa('paraglide-language-cache'), 'zh-cn');
    });
    await openMainRoute(targetPath);
    await browser.setWindowSize(1240, 638);

    // The preceding main-dropdown-menu spec deliberately leaves its menu
    // open after measuring it. Dismiss that prior suite state before checking
    // hit-testing or real pointer delivery to a Tooltip trigger.
    await browser.keys('Escape');
    await browser.waitUntil(
      async () =>
        browser.execute(
          () =>
            document.querySelector('[role="menu"][data-state="open"]') === null,
        ),
      {
        timeout: 5_000,
        timeoutMsg: 'A previous E2E dropdown menu remained open.',
      },
    );
  });

  it('keeps ref padding on the animated inner surface', async () => {
    // The Rules rail owns a real Radix Tooltip for each collapsed proxy link.
    // The Logs navigation rail has labels but does not render Tooltip roots.
    const firstSidebarLink = await $(
      '[data-slot="rules-proxy-item"][data-proxy-item-kind="all"]',
    );
    await firstSidebarLink.waitForDisplayed({ timeout: 15_000 });
    const target = await browser.execute((element) => {
      const rect = element.getBoundingClientRect();
      const x = Math.round(rect.left + rect.width / 2);
      const y = Math.round(rect.top + rect.height / 2);
      const hit = document.elementFromPoint(x, y);
      element.dataset.e2ePointerMoves = '0';
      element.dataset.e2ePointerType = 'none';
      element.dataset.e2ePointerTrusted = 'none';
      document.addEventListener(
        'pointermove',
        (event) => {
          if (event.target instanceof Node && element.contains(event.target)) {
            element.dataset.e2ePointerMoves = String(
              Number(element.dataset.e2ePointerMoves ?? 0) + 1,
            );
            element.dataset.e2ePointerType = event.pointerType;
            element.dataset.e2ePointerTrusted = String(event.isTrusted);
          }
        },
        true,
      );
      const hitElement = hit instanceof HTMLElement ? hit : null;
      return {
        x,
        y,
        hitTarget: hit === element || element.contains(hit),
        hitTag: hit?.tagName ?? null,
        hitSlot: hitElement?.dataset.slot ?? null,
        hitClass: hitElement?.className ?? null,
        targetRect: {
          x: rect.x,
          y: rect.y,
          width: rect.width,
          height: rect.height,
        },
        viewport: { width: innerWidth, height: innerHeight },
      };
    }, firstSidebarLink);
    assert.equal(target.hitTarget, true, JSON.stringify(target));

    // Move from a known safe viewport point using W3C mouse actions. The
    // embedded WebView2 driver translates those actions to synthetic DOM
    // PointerEvents; the test itself does not directly dispatch events.
    const safeX = Math.max(1, target.viewport.width - 12);
    const safeY = Math.max(
      1,
      Math.min(
        target.viewport.height - 8,
        Math.round(target.viewport.height * 0.5),
      ),
    );
    const pointer = browser.action('pointer', {
      parameters: { pointerType: 'mouse' },
    });
    await pointer
      .move({ origin: 'viewport', x: safeX, y: safeY, duration: 0 })
      .pause(100)
      .move({ origin: 'viewport', x: target.x, y: target.y, duration: 200 })
      .pause(250)
      .perform();

    try {
      await browser.waitUntil(
        async () =>
          browser.execute(
            (element) =>
              Number(element.dataset.e2ePointerMoves ?? 0) > 0 &&
              element.dataset.e2ePointerType === 'mouse' &&
              // The embedded WebView2 driver dispatches JS-created pointer
              // events; W3C actions here are not trusted OS-level input.
              element.dataset.e2ePointerTrusted === 'false' &&
              Boolean(document.querySelector<HTMLElement>('[role="tooltip"]')),
            firstSidebarLink,
          ),
        {
          timeout: 15_000,
          timeoutMsg:
            'Embedded WebDriver synthetic mouse pointer did not open the Tooltip.',
        },
      );
    } catch (cause) {
      const observed = await browser.execute((element) => {
        const rect = element.getBoundingClientRect();
        const x = Math.round(rect.left + rect.width / 2);
        const y = Math.round(rect.top + rect.height / 2);
        const hit = document.elementFromPoint(x, y);
        return {
          pointerMoves: element.dataset.e2ePointerMoves ?? null,
          pointerType: element.dataset.e2ePointerType ?? null,
          pointerTrusted: element.dataset.e2ePointerTrusted ?? null,
          triggerState: element.getAttribute('data-state'),
          centerHitsTrigger: hit === element || element.contains(hit),
          topTag: hit?.tagName ?? null,
          tooltipCount: document.querySelectorAll('[role="tooltip"]').length,
          viewport: { width: innerWidth, height: innerHeight },
        };
      }, firstSidebarLink);
      throw new Error(
        'Embedded WebDriver Tooltip hover failed: ' + JSON.stringify(observed),
        { cause },
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
        tooltipBorderRadius: tooltipStyle?.borderRadius ?? '',
        surfaceBorderRadius: surfaceStyle?.borderRadius ?? '',
        viewport: { width: innerWidth, height: innerHeight },
      };
    });

    assert.deepEqual(state.tooltipPadding, ['0px', '0px', '0px', '0px']);
    assert.deepEqual(state.surfacePadding, ['6px', '12px', '6px', '12px']);
    assert.equal(state.surfaceOverflow, 'hidden');
    assert.equal(state.tooltipChildren, 1, JSON.stringify(state, null, 2));
    assert.equal(state.surfaceChildren, 1, JSON.stringify(state, null, 2));
    assert.ok(state.contentText.length > 0, JSON.stringify(state, null, 2));
    assert.notEqual(
      state.surfaceBorderRadius,
      '0px',
      JSON.stringify(state, null, 2),
    );
    assert.ok(state.viewport.width >= 1200, JSON.stringify(state, null, 2));
    assert.ok(state.viewport.height >= 600, JSON.stringify(state, null, 2));

    const evidencePath = process.env.CHIMERA_E2E_EVIDENCE_PATH;
    if (evidencePath) {
      fs.mkdirSync(path.dirname(evidencePath), { recursive: true });
      await browser.saveScreenshot(evidencePath);
    }

    // Leave the trigger with another real pointer action. The closed Tooltip
    // must finish its exit animation and unmount the portalled content.
    await browser
      .action('pointer', { parameters: { pointerType: 'mouse' } })
      .move({ origin: 'viewport', x: safeX, y: safeY, duration: 200 })
      .perform();
    await browser.waitUntil(
      async () =>
        browser.execute(
          () => document.querySelector('[role="tooltip"]') === null,
        ),
      {
        timeout: 5_000,
        timeoutMsg: 'Tooltip portal remained mounted after real pointer exit.',
      },
    );
  });
});
