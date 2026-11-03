import assert from 'node:assert/strict';
import { clickElement, displayedElement } from './interaction.js';
import { openMainRoute } from './main-window.js';
import { markE2eStep } from './step-diagnostics.js';

const settingsPath = '/main/settings/chimera';
const settingSelector =
  '[data-slot="lighten-animation-effects-switch"] [role="switch"]';

async function openSettings() {
  markE2eStep('animation', 'open Chimera settings route');
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
    markE2eStep('animation', 'click visible setting switch');
    await clickElement(toggle);
  }

  markE2eStep('animation', 'verify visible setting switch');
  await browser.waitUntil(async () => (await isChecked()) === expected, {
    timeout: 15_000,
    timeoutMsg: `Lighten-animation setting did not become ${String(expected)}.`,
  });
}

describe('Chimera lighten-animation preference', () => {
  it('persists the setting and restores the original value', async () => {
    await openSettings();

    markE2eStep('animation', 'read initial setting');
    const original = await isChecked();
    const changed = !original;

    try {
      markE2eStep('animation', 'apply alternate setting');
      await setChecked(changed);
      markE2eStep('animation', 'verify persisted alternate setting');
      assert.equal(await readPersistedValue(), changed);
      markE2eStep('animation', 'navigate away to dashboard');
      await openMainRoute('/main/dashboard');
      await openSettings();
      assert.equal(await isChecked(), changed);
    } finally {
      markE2eStep('animation', 'restore original setting');
      await setChecked(original);
      assert.equal(await readPersistedValue(), original);
      await openMainRoute('/main/dashboard');
      await openSettings();
      assert.equal(await isChecked(), original);
    }
  });
});
