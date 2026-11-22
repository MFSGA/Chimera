import assert from 'node:assert/strict';
import fs from 'node:fs';
import path from 'node:path';
import type { MutationOutcome } from '../../frontend/interface/src/ipc/bindings.js';
import { openMainRoute } from './main-window.js';
import { committedValue } from './profile-fixtures.js';

const targetPath = '/main/settings/clash';

describe('main clash settings reference layout', () => {
  // Contract: after the fixed-window main settings route settles, the patch
  // group becomes visible and its direct card hierarchy matches the ref. Poll
  // with a fresh selector because WDIO does not wait for it to exist.
  it('uses the ref card hierarchy and excludes the system-only UWP tool', async () => {
    await browser.setWindowSize(1240, 638);
    await browser.execute(() => {
      localStorage.setItem(btoa('paraglide-language-cache'), 'zh-cn');
    });
    await openMainRoute(targetPath);
    await browser.setWindowSize(1240, 638);

    const patchGroupSelector =
      '[data-slot="patch-settings-container"] > [data-slot="settings-group"]';
    let lastObserved = 'not observed';
    try {
      await browser.waitUntil(
        async () => {
          const patchGroup = await $(patchGroupSelector);
          const exists = await patchGroup.isExisting();
          const displayed = exists && (await patchGroup.isDisplayed());
          lastObserved = JSON.stringify({ exists, displayed });
          return displayed;
        },
        {
          timeout: 15_000,
          timeoutMsg: 'The Clash patch settings group did not become visible.',
        },
      );
    } catch (error) {
      const failure = error instanceof Error ? error.message : String(error);
      throw new Error(`${failure} Last observed: ${lastObserved}.`, {
        cause: error,
      });
    }

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
        mipsControllerDirect: Boolean(
          patchGroup?.querySelector(
            ':scope > [data-slot="mips-congestion-controller-selector-card"]',
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

    // The upstream MIPS controller card is present only for the selected MIPS
    // stack. Do not mistake an additional conditional ref card for nesting.
    assert.equal(
      state.patchDirectCards,
      state.mipsControllerDirect ? 3 : 2,
      JSON.stringify(state, null, 2),
    );
    assert.equal(
      state.patchDirectChildren,
      state.mipsControllerDirect ? 5 : 4,
      JSON.stringify(state, null, 2),
    );
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

/**
 * Ref network settings flow: choose the MIPS stack in the actual main UI,
 * select a controller, read the persisted setting independently, then hand
 * control back to the core. Only the TUN-disabled E2E config is eligible.
 * No privileged TUN or host network action is part of this test.
 */
describe('Mihomo MIPS controller settings', () => {
  type AppConfig = { core: string };
  type ClashConfig = {
    tun_stack: string;
    enable_tun_mode: boolean;
    tun_mips?: { congestion_controller?: string | null };
  };

  async function invoke<T>(command: string, args?: Record<string, unknown>) {
    return browser.execute(
      async (name, parameters) => {
        const internals = (
          window as typeof window & {
            __TAURI_INTERNALS__: {
              invoke: <R>(
                name: string,
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

  async function patch(command: string, value: Record<string, unknown>) {
    committedValue(
      await invoke<MutationOutcome<null>>(command, { patch: value }),
      `MIPS E2E ${command}`,
    );
  }

  it('selects a controller and restores the core default through visible controls', async () => {
    const originalApp = await invoke<AppConfig>('get_app_config');
    const originalClash = await invoke<ClashConfig>('get_clash_config');
    assert.equal(
      originalClash.enable_tun_mode,
      false,
      'MIPS settings E2E must not run with host TUN enabled',
    );

    try {
      if (originalApp.core !== 'mihomo') {
        await patch('patch_app_config', { core: 'mihomo' });
      }
      await patch('patch_clash_config', { tun_stack: 'gvisor' });
      await openMainRoute(targetPath);

      const stackTrigger = await $('[data-slot="tun-stack-selector-trigger"]');
      await stackTrigger.waitForClickable({ timeout: 15_000 });
      await stackTrigger.click();
      const mipsOption = await $(
        '//div[@role="menuitemcheckbox" and normalize-space()="MIPS"]',
      );
      await mipsOption.waitForClickable({ timeout: 15_000 });
      await mipsOption.click();
      await browser.waitUntil(
        async () =>
          (await invoke<ClashConfig>('get_clash_config')).tun_stack === 'mips',
        {
          timeout: 20_000,
          timeoutMsg: 'Selecting MIPS did not persist the stack',
        },
      );
      const controllerTrigger = await $(
        '[data-slot="mips-congestion-controller-selector-trigger"]',
      );
      await controllerTrigger.waitForClickable({ timeout: 15_000 });
      await controllerTrigger.click();
      const bbrOption = await $(
        '//div[@role="menuitemcheckbox" and normalize-space()="BBR"]',
      );
      await bbrOption.waitForClickable({ timeout: 15_000 });
      await bbrOption.click();
      await browser.waitUntil(
        async () =>
          (await invoke<ClashConfig>('get_clash_config')).tun_mips
            ?.congestion_controller === 'bbr',
        { timeout: 20_000, timeoutMsg: 'BBR selection did not persist' },
      );

      await browser.refresh();
      const restoredLabel = await $(
        '[data-slot="mips-congestion-controller-selector-trigger"]',
      );
      await restoredLabel.waitForDisplayed({ timeout: 20_000 });
      assert.match(await restoredLabel.getText(), /BBR/);
      await restoredLabel.click();
      const defaultOption = await $(
        '//div[@role="menuitemcheckbox" and (contains(., "默认") or contains(., "預設") or contains(., "default"))]',
      );
      await defaultOption.waitForClickable({ timeout: 15_000 });
      await defaultOption.click();
      await browser.waitUntil(
        async () =>
          (await invoke<ClashConfig>('get_clash_config')).tun_mips
            ?.congestion_controller == null,
        {
          timeout: 20_000,
          timeoutMsg: 'Core default did not clear the stored MIPS controller',
        },
      );
    } finally {
      // Restore only fields changed by this test, using the same supported
      // typed mutations. Never delete another user's profiles or files.
      await patch('patch_clash_config', {
        tun_stack: originalClash.tun_stack,
        tun_mips: {
          congestion_controller:
            originalClash.tun_mips?.congestion_controller ?? null,
        },
      });
      if (originalApp.core !== 'mihomo') {
        await patch('patch_app_config', { core: originalApp.core });
      }
    }
  });
});
