import assert from 'node:assert/strict';

const aboutPath = '/main/settings/about';
const autoUpdateLabel = '自动检查更新';

async function waitForApp() {
  await browser.waitUntil(
    async () =>
      browser.execute(
        () => (document.getElementById('root')?.childElementCount ?? 0) > 0,
      ),
    { timeout: 30_000, timeoutMsg: 'The Chimera frontend did not render.' },
  );
}

async function openAboutSettings() {
  const currentHref = await browser.getUrl();
  await browser.url(new URL(aboutPath, currentHref).href);

  await browser.waitUntil(
    async () =>
      browser.execute((path) => window.location.pathname === path, aboutPath),
    {
      timeout: 15_000,
      timeoutMsg: 'About settings navigation did not complete.',
    },
  );
}

async function getAutoUpdateSwitch() {
  const label = await $(`//*[normalize-space()="${autoUpdateLabel}"]`);
  await label.waitForDisplayed({ timeout: 15_000 });

  const toggle = await $('[role="switch"]');
  await toggle.waitForClickable({ timeout: 15_000 });
  return toggle;
}

async function isChecked() {
  const toggle = await getAutoUpdateSwitch();
  return (await toggle.getAttribute('aria-checked')) === 'true';
}

async function setChecked(expected: boolean) {
  const toggle = await getAutoUpdateSwitch();
  const current = (await toggle.getAttribute('aria-checked')) === 'true';

  if (current !== expected) {
    await toggle.click();
  }

  await browser.waitUntil(async () => (await isChecked()) === expected, {
    timeout: 15_000,
    timeoutMsg: `Auto-update setting did not become ${String(expected)}.`,
  });
}

describe('Chimera auto-update preference', () => {
  it('persists the setting across a page reload and restores the original value', async () => {
    await waitForApp();
    await openAboutSettings();

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
