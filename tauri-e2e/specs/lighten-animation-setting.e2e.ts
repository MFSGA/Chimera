import assert from 'node:assert/strict';
import { clickElement, displayedElement } from './interaction.js';
import { openMainRoute } from './main-window.js';

const settingsPath = '/main/settings/chimera';
const settingSelector =
  '[data-slot="lighten-animation-effects-switch"] [role="switch"]';

async function openSettings() {
  await openMainRoute(settingsPath);

  await displayedElement(settingSelector);
}

async function isChecked() {
  const toggle = await displayedElement(settingSelector);
  return (await toggle.getAttribute('aria-checked')) === 'true';
}

async function readPersistedValue() {
  return browser.execute(async () => {
    const internals = (
      window as typeof window & {
        __TAURI_INTERNALS__: {
          invoke: (command: string) => Promise<{
            lighten_animation_effects?: boolean | null;
          }>;
        };
      }
    ).__TAURI_INTERNALS__;
    const config = await internals.invoke('get_verge_config');
    return Boolean(config.lighten_animation_effects);
  });
}

async function setChecked(expected: boolean) {
  const toggle = await displayedElement(settingSelector);
  const current = (await toggle.getAttribute('aria-checked')) === 'true';

  if (current !== expected) {
    await clickElement(toggle);
  }

  await browser.waitUntil(async () => (await isChecked()) === expected, {
    timeout: 15_000,
    timeoutMsg: `Lighten-animation setting did not become ${String(expected)}.`,
  });
}

describe('Chimera lighten-animation preference', () => {
  it('persists the setting and restores the original value', async () => {
    await openSettings();

    const original = await isChecked();
    const changed = !original;

    try {
      await setChecked(changed);
      assert.equal(await readPersistedValue(), changed);
      await openMainRoute('/main/dashboard');
      await openSettings();
      assert.equal(await isChecked(), changed);
    } finally {
      await setChecked(original);
      assert.equal(await readPersistedValue(), original);
      await openMainRoute('/main/dashboard');
      await openSettings();
      assert.equal(await isChecked(), original);
    }
  });
});
