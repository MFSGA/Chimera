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

describe('main debug settings reference layout', () => {
  before(async () => {
    await browser.setWindowSize(1240, 638);
    await browser.execute(() => {
      localStorage.setItem(btoa('paraglide-language-cache'), 'zh-cn');
    });
    await openMainWindow();
    await browser.setWindowSize(1240, 638);

    const settingsLink = await $('a[href="/main/settings/system"]');
    await settingsLink.waitForClickable({ timeout: 15_000 });
    await settingsLink.click();

    const debugLink = await $('a[href="/main/settings/debug"]');
    await debugLink.waitForClickable({ timeout: 15_000 });
    await debugLink.click();
  });

  it('uses the ref debug groups and reveals window debug tools', async () => {
    const advanced = await $('[data-slot="advanced-tools-switch-container"]');
    await advanced.waitForDisplayed({ timeout: 15_000 });

    const switchControl = await advanced.$('button[role="switch"]');
    await switchControl.click();

    const state = await browser.execute(() => ({
      path: location.pathname,
      groupCount: document.querySelectorAll(
        '[data-slot="debug-settings-container"]',
      ).length,
      hasWindowDebug: document.body.innerText.includes('Window Debug Utils'),
      hasWindowLabel: document.body.innerText.includes('Current Window Label:'),
      hasEditorButton: document.body.innerText.includes(
        'Create Test Editor Window',
      ),
      hasTrayButton: document.body.innerText.includes(
        'Create Persistent Tray Menu Window',
      ),
    }));

    assert.equal(state.path, '/main/settings/debug');
    assert.equal(state.groupCount, 2, JSON.stringify(state, null, 2));
    assert.equal(state.hasWindowDebug, true, JSON.stringify(state, null, 2));
    assert.equal(state.hasWindowLabel, true, JSON.stringify(state, null, 2));
    assert.equal(state.hasEditorButton, true, JSON.stringify(state, null, 2));
    assert.equal(state.hasTrayButton, true, JSON.stringify(state, null, 2));

    const evidencePath = process.env.CHIMERA_E2E_EVIDENCE_PATH;
    if (evidencePath) {
      fs.mkdirSync(path.dirname(evidencePath), { recursive: true });
      await browser.saveScreenshot(evidencePath);
    }
  });
});
