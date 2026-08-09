import assert from 'node:assert/strict';

const settingsPath = '/main/settings/nyanpasu';
const settingSelector =
  '[data-slot="app-settings-container"]:last-child [role="switch"]';

async function waitForApp() {
  await browser.waitUntil(
    async () =>
      browser.execute(
        () => (document.getElementById('root')?.childElementCount ?? 0) > 0,
      ),
    { timeout: 30_000, timeoutMsg: 'The Chimera frontend did not render.' },
  );
}

async function openSettings() {
  const currentHref = await browser.getUrl();
  await browser.url(new URL(settingsPath, currentHref).href);
  await waitForApp();

  const toggle = await $(settingSelector);
  await toggle.waitForDisplayed({ timeout: 15_000 });
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
    await waitForApp();
    await openSettings();

    const original = await isChecked();
    const changed = !original;

    try {
      await setChecked(changed);
      await browser.refresh();
      await waitForApp();
      assert.equal(await isChecked(), changed);
    } finally {
      await setChecked(original);
      await browser.refresh();
      await waitForApp();
      assert.equal(await isChecked(), original);
    }
  });
});
