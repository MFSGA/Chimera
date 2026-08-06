import assert from 'node:assert/strict';

const profilesPath = '/main/profiles/profile';

async function openLocalProfileForm() {
  const currentUrl = new URL(await browser.getUrl());
  currentUrl.pathname = profilesPath;
  currentUrl.search = '';
  currentUrl.searchParams.set('action', 'ImportLocalProfile');
  await browser.url(currentUrl.href);

  await browser.waitUntil(
    async () =>
      browser.execute(
        () => (document.getElementById('root')?.childElementCount ?? 0) > 0,
      ),
    { timeout: 15_000, timeoutMsg: 'The profiles page did not render.' },
  );

  const nameInput = await $('input[name="name"]');
  await nameInput.waitForDisplayed({ timeout: 15_000 });
  return nameInput;
}

async function expectNoProfileCards() {
  assert.equal((await $$('[data-slot="profile-card"]')).length, 0);
}

describe('Chimera local profile draft', () => {
  it('cancels a local profile draft without persisting it', async () => {
    const draftName = `local-draft-${Date.now()}`;
    const nameInput = await openLocalProfileForm();
    await expectNoProfileCards();
    await nameInput.setValue(draftName);

    const descriptionInput = await $('textarea[name="desc"]');
    await descriptionInput.setValue('This draft must be discarded.');

    const closeButton = await $('button=Close');
    await closeButton.waitForClickable({ timeout: 15_000 });
    await closeButton.click();

    await browser.waitUntil(async () => !(await nameInput.isExisting()), {
      timeout: 15_000,
      timeoutMsg: 'The local profile dialog did not close.',
    });
    assert.equal((await $('body').getText()).includes(draftName), false);
    await expectNoProfileCards();

    await browser.refresh();
    await browser.waitUntil(
      async () =>
        browser.execute(
          (name) => !document.body.innerText.includes(name),
          draftName,
        ),
      {
        timeout: 15_000,
        timeoutMsg:
          'The cancelled local profile draft was persisted after reload.',
      },
    );
    await expectNoProfileCards();
  });

  it('keeps a local profile with an empty name invalid and unpersisted', async () => {
    const nameInput = await openLocalProfileForm();
    await nameInput.setValue('');

    const okButton = await $('button=OK');
    await okButton.waitForClickable({ timeout: 15_000 });
    await okButton.click();

    await browser.waitUntil(
      async () => (await nameInput.getAttribute('aria-invalid')) === 'true',
      {
        timeout: 15_000,
        timeoutMsg: 'The empty profile name was not marked invalid.',
      },
    );
    assert.equal(await nameInput.isDisplayed(), true);
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
