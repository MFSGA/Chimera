import assert from 'node:assert/strict';
import fs from 'node:fs';
import path from 'node:path';

const targetPath = '/main/settings/clash';

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

describe('main clash settings reference layout', () => {
  it('uses the ref card hierarchy and excludes the system-only UWP tool', async () => {
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
        browser.execute((target) => location.pathname === target, targetPath),
      { timeout: 15_000, timeoutMsg: 'Clash settings route did not open.' },
    );

    const patch = await $('[data-slot="patch-settings-container"]');
    await patch.waitForDisplayed({ timeout: 15_000 });

    const state = await browser.execute(() => {
      const patchGroup = document.querySelector(
        '[data-slot="patch-settings-container"] [data-slot="settings-group"]',
      );
      const portGroup = document.querySelector(
        '[data-slot="port-settings-container"] [data-slot="settings-group"]',
      );
      const fieldContainer = document.querySelector(
        '[data-slot="field-filter-settings-container"] > .space-y-2',
      );
      const nestedCards = document.querySelectorAll(
        '[data-slot="settings-card"] [data-slot="settings-card"]',
      );
      const directCards = (group: Element | null) =>
        group
          ? Array.from(group.children).filter(
              (child) => child.getAttribute('data-slot') === 'settings-card',
            ).length
          : 0;

      return {
        patchDirectCards: directCards(patchGroup),
        patchDirectChildren: patchGroup?.children.length ?? 0,
        tunStackDirect: Boolean(
          patchGroup?.querySelector(
            ':scope > [data-slot="tun-stack-selector-card"]',
          ),
        ),
        logLevelDirect: Boolean(
          patchGroup?.querySelector(
            ':scope > [data-slot="log-level-selector-card"]',
          ),
        ),
        portDirectCards: directCards(portGroup),
        fieldDirectCards: directCards(fieldContainer),
        fieldHasSettingsGroup: Boolean(
          document.querySelector(
            '[data-slot="field-filter-settings-container"] [data-slot="settings-group"]',
          ),
        ),
        nestedCardCount: nestedCards.length,
        allowLanInsideCard: Boolean(
          document.querySelector(
            '[data-slot="settings-card"] [data-slot="allow-lan-switch-container"]',
          ),
        ),
        ipv6InsideCard: Boolean(
          document.querySelector(
            '[data-slot="settings-card"] [data-slot="ipv6-switch-container"]',
          ),
        ),
        randomPortInsideCard: Boolean(
          document.querySelector(
            '[data-slot="settings-card"] [data-slot="random-port-switch-container"]',
          ),
        ),
        fieldFilterInsideCard: Boolean(
          document.querySelector(
            '[data-slot="settings-card"] [data-slot="field-filter-switch-container"]',
          ),
        ),
        viewport: { width: innerWidth, height: innerHeight },
      };
    });

    assert.equal(state.patchDirectCards, 2, JSON.stringify(state, null, 2));
    assert.equal(state.patchDirectChildren, 4, JSON.stringify(state, null, 2));
    assert.equal(state.tunStackDirect, true, JSON.stringify(state, null, 2));
    assert.equal(state.logLevelDirect, true, JSON.stringify(state, null, 2));
    assert.equal(state.portDirectCards, 1, JSON.stringify(state, null, 2));
    assert.equal(state.fieldDirectCards, 1, JSON.stringify(state, null, 2));
    assert.equal(
      state.fieldHasSettingsGroup,
      false,
      JSON.stringify(state, null, 2),
    );
    assert.equal(state.nestedCardCount, 0, JSON.stringify(state, null, 2));
    assert.equal(
      state.allowLanInsideCard,
      true,
      JSON.stringify(state, null, 2),
    );
    assert.equal(state.ipv6InsideCard, true, JSON.stringify(state, null, 2));
    assert.equal(
      state.randomPortInsideCard,
      true,
      JSON.stringify(state, null, 2),
    );
    assert.equal(
      state.fieldFilterInsideCard,
      true,
      JSON.stringify(state, null, 2),
    );

    const evidencePath = process.env.CHIMERA_E2E_EVIDENCE_PATH;
    if (evidencePath) {
      fs.mkdirSync(path.dirname(evidencePath), { recursive: true });
      await browser.saveScreenshot(evidencePath);
    }
  });
});
