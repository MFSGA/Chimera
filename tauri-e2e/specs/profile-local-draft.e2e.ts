import assert from 'node:assert/strict';

const profilesPath = '/main/profiles/profile';

type ProfilesResponse = {
  items: Array<{ uid: string }>;
};

async function readProfileIds(): Promise<string[]> {
  return browser.execute(async () => {
    const internals = (
      window as typeof window & {
        __TAURI_INTERNALS__: {
          invoke: <T>(command: string) => Promise<T>;
        };
      }
    ).__TAURI_INTERNALS__;
    const profiles = await internals.invoke<ProfilesResponse>('get_profiles');
    return profiles.items.map((item) => item.uid).sort();
  });
}

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

async function expectProfileIds(expected: string[]) {
  assert.deepEqual(await readProfileIds(), expected);
}

describe('Chimera local profile draft', () => {
  it('cancels a local profile draft without persisting it', async () => {
    const draftName = `local-draft-${Date.now()}`;
    const nameInput = await openLocalProfileForm();
    const initialProfileIds = await readProfileIds();
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
    await expectProfileIds(initialProfileIds);

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
    await expectProfileIds(initialProfileIds);
  });

  it('keeps a local profile with an empty name invalid and unpersisted', async () => {
    const nameInput = await openLocalProfileForm();
    const initialProfileIds = await readProfileIds();
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
    await expectProfileIds(initialProfileIds);

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
    await expectProfileIds(initialProfileIds);
  });
});
