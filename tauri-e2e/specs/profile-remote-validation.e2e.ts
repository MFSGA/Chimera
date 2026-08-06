import assert from 'node:assert/strict';

const profilesPath = '/main/profiles/profile';

async function openRemoteProfileForm() {
  const currentUrl = new URL(await browser.getUrl());
  currentUrl.pathname = profilesPath;
  currentUrl.search = '';
  await browser.url(currentUrl.href);

  await browser.waitUntil(
    async () =>
      browser.execute(
        () => (document.getElementById('root')?.childElementCount ?? 0) > 0,
      ),
    { timeout: 15_000, timeoutMsg: 'The profiles page did not render.' },
  );

  const importButton = await $('[data-slot="profile-import-button"]');
  await importButton.waitForDisplayed({ timeout: 15_000 });

  const createButton = await importButton.$(
    'button:not([data-profile-import-type])',
  );
  await createButton.waitForClickable({ timeout: 15_000 });
  await createButton.click();

  await browser.waitUntil(
    async () => (await importButton.getAttribute('data-expanded')) === 'true',
    {
      timeout: 15_000,
      timeoutMsg: 'The profile import menu did not expand.',
    },
  );

  const remoteImportButton = await importButton.$(
    '[data-profile-import-type="remote"]',
  );
  await remoteImportButton.waitForClickable({ timeout: 15_000 });
  await remoteImportButton.click();

  const urlInput = await $('textarea[name="url"]');
  await urlInput.waitForDisplayed({ timeout: 15_000 });
  return urlInput;
}

async function expectNoProfileCards() {
  assert.equal((await $$('[data-slot="profile-card"]')).length, 0);
}

describe('Chimera remote profile validation', () => {
  it('keeps a remote profile with an empty URL invalid and unpersisted', async () => {
    const urlInput = await openRemoteProfileForm();
    await urlInput.setValue('');

    const okButton = await $('button=OK');
    await okButton.waitForClickable({ timeout: 15_000 });
    await okButton.click();

    await browser.waitUntil(
      async () => (await urlInput.getAttribute('aria-invalid')) === 'true',
      {
        timeout: 15_000,
        timeoutMsg: 'The empty subscription URL was not marked invalid.',
      },
    );
    assert.equal(await urlInput.isDisplayed(), true);
    await expectNoProfileCards();

    const closeButton = await $('button=Close');
    await closeButton.click();
    await browser.refresh();
    await browser.waitUntil(
      async () =>
        browser.execute(
          () => (document.getElementById('root')?.childElementCount ?? 0) > 0,
        ),
      { timeout: 15_000, timeoutMsg: 'The profiles page did not reload.' },
    );
    await expectNoProfileCards();
  });
});
