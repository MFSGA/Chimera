import assert from 'node:assert/strict';
import fs from 'node:fs';
import path from 'node:path';
import {
  createProfile,
  invoke,
  localConfigProfileRequest,
} from './profile-fixtures.js';

const profileName = `Main Detail Ref Profile ${Date.now()}`;

type ProfilesResponse = {
  items: Array<{ name: string; uid: string }>;
};

async function openMainWindow() {
  await invoke('create_main_window');
  await browser.waitUntil(
    async () => (await browser.getWindowHandles()).includes('main'),
    { timeout: 15_000, timeoutMsg: 'The main window was not created.' },
  );
  await browser.switchToWindow('main');
}

async function getActiveClickableElement(selector: string) {
  await browser.waitUntil(
    async () => {
      const elements = await $$(selector);
      const elementCount = await elements.length;
      for (let index = elementCount - 1; index >= 0; index -= 1) {
        if (await elements[index].isClickable().catch(() => false)) return true;
      }
      return false;
    },
    {
      timeout: 15_000,
      timeoutMsg: `No active clickable element matched ${selector}.`,
    },
  );

  const elements = await $$(selector);
  const elementCount = await elements.length;
  for (let index = elementCount - 1; index >= 0; index -= 1) {
    if (await elements[index].isClickable().catch(() => false)) {
      return elements[index];
    }
  }

  throw new Error(`No active clickable element matched ${selector}.`);
}

describe('main profile detail reference editors', () => {
  let profileUid: string | undefined;

  before(async () => {
    await browser.setWindowSize(1240, 638);

    const created = await createProfile(
      localConfigProfileRequest(profileName),
      'mixed-port: 27890\nmode: rule\n',
    );
    profileUid = created.value;

    const profiles = await invoke<ProfilesResponse>('get_profiles');
    assert.ok(
      profiles.items.some((item) => item.uid === profileUid),
      'The isolated detail profile was not persisted.',
    );

    await openMainWindow();
    await browser.setWindowSize(1240, 638);

    const appHeader = await $('[data-slot="app-header"]');
    await appHeader.waitForDisplayed({ timeout: 15_000 });

    const detailPath = `/main/profiles/profile/detail/${profileUid}`;
    await browser.execute((target) => {
      history.pushState({}, '', target);
      window.dispatchEvent(new PopStateEvent('popstate'));
    }, detailPath);

    await browser.waitUntil(
      async () =>
        browser.execute(
          (expected) =>
            location.pathname === expected &&
            Boolean(document.querySelector('[data-slot="profile-detail"]')),
          detailPath,
        ),
      {
        timeout: 30_000,
        timeoutMsg: 'The main profile detail route did not render.',
      },
    );
  });

  after(async () => {
    const uid =
      profileUid ??
      (await invoke<ProfilesResponse>('get_profiles')).items.find(
        (item) => item.name === profileName,
      )?.uid;
    if (!uid) return;
    await invoke('delete_profile', { uid });
  });

  // Contract: a locally created file Profile opens in the main detail route;
  // clearing and saving invalid metadata must preserve the last valid value
  // and expose the inline error within the ref field wrapper. The generated
  // Profile request and authoritative document make obsolete IPC fail setup.
  it('uses the ref field wrapper and animated validation error', async () => {
    const editButton = await getActiveClickableElement(
      '[data-slot="profile-name-edit"]',
    );
    await editButton.click();

    const modal = await $('[data-slot="modal-content"]');
    await modal.waitForDisplayed({ timeout: 15_000 });

    const input = await modal.$('input');
    await input.waitForDisplayed({ timeout: 15_000 });
    await input.setValue('');

    const saveButton = await modal.$('button');
    await saveButton.waitForClickable({ timeout: 15_000 });
    await saveButton.click();

    await browser.waitUntil(
      async () =>
        browser.execute(() => {
          const modalContent = document.querySelector<HTMLElement>(
            '[data-slot="modal-content"]',
          );
          return Boolean(modalContent?.querySelector('.text-error'));
        }),
      {
        timeout: 15_000,
        timeoutMsg: 'The profile name validation error did not render.',
      },
    );

    await browser.waitUntil(
      async () =>
        browser.execute(() => {
          const modalContent = document.querySelector<HTMLElement>(
            '[data-slot="modal-content"]',
          );
          const card = modalContent?.querySelector<HTMLElement>(
            '[data-slot="card-root"]',
          );
          const error = modalContent?.querySelector<HTMLElement>('.text-error');
          if (!modalContent || !card || !error) return false;
          const style = getComputedStyle(error);
          return (
            card.getBoundingClientRect().width === 384 &&
            error.getBoundingClientRect().height > 0 &&
            style.overflow === 'hidden' &&
            Number(style.opacity) === 1
          );
        }),
      {
        timeout: 5_000,
        timeoutMsg: 'The profile validation field animation did not settle.',
      },
    );

    const card = await modal.$('[data-slot="card-root"]');
    const error = await modal.$('.text-error');
    const cardWidth = await card.getCSSProperty('width');
    const errorHeight = await error.getSize('height');
    const errorOverflow = await error.getCSSProperty('overflow');
    const errorOpacity = await error.getCSSProperty('opacity');
    const state = await browser.execute(() => ({
      viewport: { width: innerWidth, height: innerHeight },
      wrapperClass:
        document
          .querySelector<HTMLElement>('[data-slot="modal-content"] input')
          ?.parentElement?.parentElement?.getAttribute('class') ?? '',
    }));

    assert.ok(state.viewport.width >= 1200, JSON.stringify(state, null, 2));
    assert.ok(state.viewport.height >= 600, JSON.stringify(state, null, 2));
    assert.equal(cardWidth.value, '384px');
    assert.match(state.wrapperClass, /space-y-2/);
    assert.ok(
      errorHeight > 0,
      `Expected visible error height, got ${errorHeight}`,
    );
    assert.equal(errorOverflow.value, 'hidden');
    assert.equal(errorOpacity.value, 1);
    const profiles = await invoke<ProfilesResponse>('get_profiles');
    assert.equal(
      profiles.items.find((item) => item.uid === profileUid)?.name,
      profileName,
      'Invalid metadata must not replace the persisted Profile name.',
    );

    const evidencePath = process.env.CHIMERA_E2E_EVIDENCE_PATH;
    if (evidencePath) {
      fs.mkdirSync(path.dirname(evidencePath), { recursive: true });
      await browser.saveScreenshot(evidencePath);
    }
  });
});
