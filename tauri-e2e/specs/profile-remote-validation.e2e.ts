import assert from 'node:assert/strict';
import { openMainRoute } from './main-window.js';

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

async function openRemoteProfileForm() {
  await browser.setWindowSize(1240, 638);
  await openMainRoute(profilesPath);
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

  const createButton = await $('[data-slot="profile-import-toggle"]');
  await createButton.waitForClickable({ timeout: 15_000 });
  await createButton.click();

  const remoteImportButton = await $(
    '[data-slot="profile-import-remote-action"]',
  );
  await remoteImportButton.waitForDisplayed({
    timeout: 15_000,
    timeoutMsg: 'The profile import menu did not expand.',
  });
  await remoteImportButton.waitForClickable({ timeout: 15_000 });
  await remoteImportButton.click();

  const urlInput = await $('textarea[name="url"]');
  await urlInput.waitForDisplayed({ timeout: 15_000 });
  return urlInput;
}

async function expectProfileIds(expected: string[]) {
  assert.deepEqual(await readProfileIds(), expected);
}

describe('Chimera remote profile validation', () => {
  // Contract: with the main Profile list loaded, choosing remote import and
  // submitting an empty URL must expose field validation without changing the
  // Profile UID set, including after reload. The selector assertions fail if
  // the import menu cannot be opened or validation is removed.
  it('keeps a remote profile with an empty URL invalid and unpersisted', async () => {
    const urlInput = await openRemoteProfileForm();
    const initialProfileIds = await readProfileIds();
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
