import assert from 'node:assert/strict';
import fs from 'node:fs';
import path from 'node:path';

const targetPath = '/main/logs';

async function invoke<T>(command: string, args?: Record<string, unknown>) {
  return browser.execute(
    async (name, parameters) => {
      const internals = (
        window as typeof window & {
          __TAURI_INTERNALS__: {
            invoke: <R>(
              command: string,
              args?: Record<string, unknown>,
            ) => Promise<R>;
          };
        }
      ).__TAURI_INTERNALS__;
      return internals.invoke<T>(name, parameters);
    },
    command,
    args,
  );
}

async function openMainWindow() {
  await invoke('create_main_window');
  await browser.waitUntil(
    async () => (await browser.getWindowHandles()).includes('main'),
    { timeout: 15_000, timeoutMsg: 'The main window was not created.' },
  );
  await browser.switchToWindow('main');
}

describe('main tooltip reference surface', () => {
  before(async () => {
    await browser.setWindowSize(1240, 638);
    await browser.execute(() => {
      localStorage.setItem(btoa('paraglide-language-cache'), 'zh-cn');
    });
    await openMainWindow();
    await browser.setWindowSize(1240, 638);

    await browser.execute((target) => {
      history.pushState({}, '', target);
      window.dispatchEvent(new PopStateEvent('popstate'));
    }, targetPath);
    await browser.waitUntil(
      async () =>
        browser.execute(
          (expected) => location.pathname === expected,
          targetPath,
        ),
      { timeout: 15_000, timeoutMsg: 'Logs route did not open.' },
    );
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
