import assert from 'node:assert/strict';
import fs from 'node:fs';
import path from 'node:path';

const profileName = 'Main Detail Ref Profile';

type ProfilesResponse = {
  items: Array<{ name: string; uid: string }>;
};

async function invoke<T>(command: string, args?: Record<string, unknown>) {
  return browser.execute(
    async (name, parameters) => {
      const internals = (
        window as typeof window & {
          __TAURI_INTERNALS__: {
            invoke: <R>(
              command: string,
              args?: Record<string, unknown>,
            ) => Promise<R>;
          };
        }
      ).__TAURI_INTERNALS__;
      return internals.invoke<T>(name, parameters);
    },
    command,
    args,
  );
}

async function openMainWindow() {
  await invoke('create_main_window');
  await browser.waitUntil(
    async () => (await browser.getWindowHandles()).includes('main'),
    { timeout: 15_000, timeoutMsg: 'The main window was not created.' },
  );
  await browser.switchToWindow('main');
}

describe('main profile detail reference editors', () => {
  let profileUid: string | undefined;

  before(async () => {
    await browser.setWindowSize(1240, 638);

    const existing = await invoke<ProfilesResponse>('get_profiles');
    const stale = existing.items.find((item) => item.name === profileName);
    if (stale) {
      await invoke('delete_profile', { uid: stale.uid }).catch(() => undefined);
    }

    await invoke('create_profile', {
      item: {
        type: 'local',
        uid: null,
        name: profileName,
        file: null,
        desc: null,
        updated: null,
        symlinks: null,
        chain: null,
      },
      fileData: 'mixed-port: 27890\nmode: rule\n',
    });

    const profiles = await invoke<ProfilesResponse>('get_profiles');
    profileUid = profiles.items.find((item) => item.name === profileName)?.uid;
    assert.ok(profileUid, 'The isolated detail profile was not created.');

    await browser.execute(() => {
      localStorage.setItem(btoa('paraglide-language-cache'), 'zh-cn');
    });
    await openMainWindow();
    await browser.setWindowSize(1240, 638);

    const currentUrl = new URL(await browser.getUrl());
    currentUrl.pathname = `/main/profiles/profile/detail/${profileUid}`;
    currentUrl.search = '';
    await browser.url(currentUrl.href);

    await browser.waitUntil(
      async () =>
        browser.execute(
          (expected) =>
            location.pathname === expected &&
            document.body.innerText.includes('Main Detail Ref Profile'),
          `/main/profiles/profile/detail/${profileUid}`,
        ),
      {
        timeout: 15_000,
        timeoutMsg: 'The main profile detail route did not render.',
      },
    );
  });

  after(async () => {
    if (!profileUid) return;
    await invoke('delete_profile', { uid: profileUid }).catch(() => undefined);
  });

  it('uses the ref field wrapper and animated validation error', async () => {
    const editButton = await $('div.sticky button');
    await editButton.waitForClickable({ timeout: 15_000 });
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

    await browser.pause(250);

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

    const evidencePath = process.env.CHIMERA_E2E_EVIDENCE_PATH;
    if (evidencePath) {
      fs.mkdirSync(path.dirname(evidencePath), { recursive: true });
      await browser.saveScreenshot(evidencePath);
    }
  });
});
