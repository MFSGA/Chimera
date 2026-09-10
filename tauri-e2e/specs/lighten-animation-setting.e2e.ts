import assert from 'node:assert/strict';
import { openMainRoute } from './main-window.js';

const settingsPath = '/main/settings/chimera';
const settingSelector =
  '[data-slot="app-settings-container"]:last-child [role="switch"]';

async function openSettings() {
  await openMainRoute(settingsPath);

  const toggle = await $(settingSelector);
  await toggle.waitForDisplayed({ timeout: 15_000 });
  await toggle.scrollIntoView({ block: 'center' });
  await toggle.waitForClickable({ timeout: 15_000 });
}

async function isChecked() {
  const toggle = await $(settingSelector);
  return (await toggle.getAttribute('aria-checked')) === 'true';
}

async function setChecked(expected: boolean) {
  const toggle = await $(settingSelector);
  const current = (await toggle.getAttribute('aria-checked')) === 'true';

  if (current !== expected) {
    await toggle.click();
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
      await browser.refresh();
      await openSettings();
      assert.equal(await isChecked(), changed);
    } finally {
      await setChecked(original);
      await browser.refresh();
      await openSettings();
      assert.equal(await isChecked(), original);
    }
  });
});
