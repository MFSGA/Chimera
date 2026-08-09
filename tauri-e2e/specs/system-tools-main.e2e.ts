import assert from 'node:assert/strict';
import fs from 'node:fs';
import path from 'node:path';

const targetPath = '/main/settings/system';

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

describe('main system tools reference layout', () => {
  before(async () => {
    await browser.setWindowSize(1240, 638);
    await browser.execute(() => {
      localStorage.setItem(btoa('paraglide-language-cache'), 'zh-cn');
    });
    await openMainWindow();
    await browser.setWindowSize(1240, 638);

    const link = await $(`a[href="${targetPath}"]`);
    await link.waitForClickable({ timeout: 15_000 });
    await link.click();
    await browser.waitUntil(
      async () =>
        browser.execute(
          (expected) => location.pathname === expected,
          targetPath,
        ),
      { timeout: 15_000, timeoutMsg: 'System settings route did not open.' },
    );

    const tools = await $('[data-slot="system-tools-container"]');
    await tools.waitForDisplayed({ timeout: 15_000 });
  });

  it('shows the ref-style Windows UWP tools card', async () => {
    const state = await browser.execute(() => {
      const tools = document.querySelector<HTMLElement>(
        '[data-slot="system-tools-container"]',
      );
      const card = document.querySelector<HTMLElement>(
        '[data-slot="uwp-tools-button-card"]',
      );
      const button = card?.querySelector<HTMLButtonElement>('button');
      const label = card?.querySelector<HTMLElement>(
        '[data-slot="settings-card-content-item-label-text"]',
      );
      const description = card?.querySelector<HTMLElement>(
        '[data-slot="settings-card-content-item-label-description"]',
      );
      const rect = (element: HTMLElement | null | undefined) =>
        element
          ? {
              width: Math.round(element.getBoundingClientRect().width),
              height: Math.round(element.getBoundingClientRect().height),
            }
          : null;

      return {
        tools: rect(tools),
        card: rect(card),
        button: rect(button),
        label: label?.textContent?.trim() ?? '',
        description: description?.textContent?.trim() ?? '',
        buttonTextAlign: button ? getComputedStyle(button).textAlign : null,
      };
    });

    assert.ok(state.tools, JSON.stringify(state, null, 2));
    assert.ok(state.card, JSON.stringify(state, null, 2));
    assert.ok(state.button, JSON.stringify(state, null, 2));
    assert.ok(state.label.length > 0, JSON.stringify(state, null, 2));
    assert.ok(state.description.length > 0, JSON.stringify(state, null, 2));
    assert.equal(state.buttonTextAlign, 'left', JSON.stringify(state, null, 2));
    assert.ok(
      (state.button?.height ?? 0) >= 56,
      JSON.stringify(state, null, 2),
    );

    const evidencePath = process.env.CHIMERA_E2E_EVIDENCE_PATH;
    if (evidencePath) {
      fs.mkdirSync(path.dirname(evidencePath), { recursive: true });
      await browser.saveScreenshot(evidencePath);
    }
  });
});
