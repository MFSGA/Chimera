import assert from 'node:assert/strict';
import { displayedElement } from './interaction.js';
import { openMainRoute } from './main-window.js';
import { invoke, withCleanup } from './profile-fixtures.js';

const aboutPath = '/main/settings/about';
const switchSelector = '[data-slot="auto-check-updates-switch"][role="switch"]';

interface AppConfig {
  enable_auto_check_update: boolean;
}

async function readPersistedSetting(): Promise<boolean> {
  const config = await invoke<AppConfig>('get_app_config');
  assert.equal(
    typeof config.enable_auto_check_update,
    'boolean',
    'Expected a typed auto-update preference from the backend.',
  );
  return config.enable_auto_check_update;
}

async function readVisibleSetting(): Promise<boolean> {
  const toggle = await displayedElement(switchSelector);
  const checked = await toggle.getAttribute('aria-checked');
  assert.ok(
    checked === 'true' || checked === 'false',
    'Switch must expose checked state.',
  );
  return checked === 'true';
}

async function waitForSetting(
  expected: boolean,
  source: 'backend' | 'ui',
): Promise<void> {
  let observed: boolean | null = null;
  try {
    await browser.waitUntil(
      async () => {
        observed =
          source === 'backend'
            ? await readPersistedSetting()
            : await readVisibleSetting();
        return observed === expected;
      },
      {
        timeout: 15_000,
        interval: 250,
        timeoutMsg: `The ${source} auto-update setting did not reach ${expected}.`,
      },
    );
  } catch (cause) {
    throw new Error(
      `State-change sync failed: ${source} expected ${expected}, last observed ${observed}.`,
      { cause },
    );
  }
}

async function patchPreference(expected: boolean): Promise<void> {
  await invoke('patch_app_config', {
    patch: { enable_auto_check_update: expected },
  });
}

// Contract (T01/T02/T06):
// - An already mounted main settings page must reflect a committed external
//   typed configuration mutation without reloading/navigating.
// - Clicking the actual switch must first commit to the backend; then a separate
//   backend mutation must update that same mounted switch through StateChangedEvent.
// - A missing event, stale query or old in-flight read must leave the UI incorrect
//   and cause the second observation to fail. Backend IPC is only used to prepare
//   the external change and read the committed state, never to fake UI success.
// - This covers desktop UI/cache delivery, not network behavior or cold-start persistence.
describe('StateChangedEvent: main UI configuration synchronization', () => {
  it('refreshes a mounted settings switch after another caller commits', async () => {
    await withCleanup('typed state change notification', async (defer) => {
      await openMainRoute(aboutPath);
      await displayedElement(switchSelector);

      const original = await readPersistedSetting();
      assert.equal(
        await readVisibleSetting(),
        original,
        'The mounted settings view must agree with the backend at baseline.',
      );
      defer('restore original auto-check-updates preference', async () => {
        await patchPreference(original);
        await waitForSetting(original, 'backend');
        await waitForSetting(original, 'ui');
      });

      const changed = !original;
      const toggle = await displayedElement(switchSelector);
      await toggle.click();
      await waitForSetting(changed, 'backend');
      await waitForSetting(changed, 'ui');

      // Simulate another caller (hotkey/agent/another window) changing the
      // same shared domain while the page stays mounted. No refresh allowed.
      await patchPreference(original);
      await waitForSetting(original, 'backend');
      await waitForSetting(original, 'ui');
    });
  });
});
