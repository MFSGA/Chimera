import assert from 'node:assert/strict';
import fs from 'node:fs';
import path from 'node:path';

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

describe('main ref dropdown menu', () => {
  it('uses the ref menu geometry without changing the legacy primitive', async () => {
    await browser.setWindowSize(1240, 638);
    await browser.execute(() => {
      localStorage.setItem(btoa('paraglide-language-cache'), 'zh-cn');
    });
    await openMainWindow();
    await browser.setWindowSize(1240, 638);

    const appHeader = await $('[data-slot="app-header"]');
    const settingsButton = await appHeader.$('button=设置');
    await settingsButton.waitForClickable({ timeout: 15_000 });
    await settingsButton.click();

    const openState = await browser.execute(() => {
      const trigger = Array.from(
        document.querySelectorAll<HTMLButtonElement>(
          '[data-slot="app-header"] button',
        ),
      ).find((button) => button.textContent?.trim() === '设置');
      return {
        triggerState: trigger?.getAttribute('data-state') ?? '',
        roleMenuCount: document.querySelectorAll('[role="menu"]').length,
        bodyText: document.body.innerText,
      };
    });
    console.log('main dropdown open state', openState);

    const evidencePath = process.env.CHIMERA_E2E_EVIDENCE_PATH;
    if (evidencePath) {
      fs.mkdirSync(path.dirname(evidencePath), { recursive: true });
      await browser.saveScreenshot(evidencePath);
    }

    assert.equal(
      openState.triggerState,
      'open',
      JSON.stringify(openState, null, 2),
    );

    const content = await $('[data-slot="main-dropdown-menu-motion-content"]');
    await content.waitForDisplayed({ timeout: 15_000 });

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
          Math.round(item.getBoundingClientRect().height),
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
