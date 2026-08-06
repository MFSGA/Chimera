import assert from 'node:assert/strict';
import fs from 'node:fs';
import path from 'node:path';
import { fileURLToPath } from 'node:url';

const profileName = 'TDD 本地配置';
const fixturePath = path.resolve(
  path.dirname(fileURLToPath(import.meta.url)),
  '../fixtures/proxy-localization.yaml',
);
const fixture = fs.readFileSync(fixturePath, 'utf8');

type ProfilesResponse = {
  current: string | null;
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

async function waitForPath(pathname: string) {
  await browser.waitUntil(
    async () =>
      browser.execute((expected) => location.pathname === expected, pathname),
    {
      timeout: 15_000,
      timeoutMsg: `Navigation to ${pathname} did not complete.`,
    },
  );
}

describe('legacy proxy localization', () => {
  let profileUid: string | undefined;

  before(async () => {
    await browser.setWindowSize(1240, 638);
    await browser.waitUntil(
      async () =>
        browser.execute(
          () =>
            document.readyState === 'complete' &&
            (document.getElementById('root')?.childElementCount ?? 0) > 0,
        ),
      { timeout: 30_000, timeoutMsg: 'The Chimera frontend did not render.' },
    );

    await browser.execute(() => {
      localStorage.setItem(btoa('paraglide-language-cache'), 'zh-cn');
      localStorage.setItem(
        'memorizedRoutePathAtom',
        JSON.stringify('/profiles'),
      );
    });
    await browser.refresh();
  });

  after(async () => {
    if (!profileUid) return;
    await invoke('activate_profile', { uid: null }).catch(() => undefined);
    await invoke('delete_profile', { uid: profileUid }).catch(() => undefined);
  });

  it('localizes the proxy page after creating and activating a local profile', async () => {
    const profilesLink = await $('//*[normalize-space()="配置"]');
    await profilesLink.waitForClickable({ timeout: 15_000 });
    await profilesLink.click();
    await waitForPath('/profiles');

    const addButton = await $('button.MuiFab-primary');
    await addButton.waitForClickable();
    await addButton.click();

    await (await $('//*[normalize-space()="创建配置"]')).waitForDisplayed();
    const typeSelect = await $('[role="combobox"]');
    await typeSelect.waitForClickable();
    await browser.execute(() =>
      document
        .querySelector<HTMLElement>('[role="combobox"]')
        ?.dispatchEvent(
          new MouseEvent('mousedown', { bubbles: true, button: 0 }),
        ),
    );
    const localOption = await $('[role="option"][data-value="local"]');
    await localOption.waitForClickable();
    await localOption.click();

    const nameInput = await $('input[name="name"]');
    await nameInput.setValue(profileName);
    const confirmButton = await $('//button[normalize-space()="OK"]');
    await confirmButton.click();

    const profileNameElement = await $(
      `//*[normalize-space()="${profileName}"]`,
    );
    await profileNameElement.waitForDisplayed({ timeout: 15_000 });

    const profiles = await invoke<ProfilesResponse>('get_profiles');
    profileUid = profiles.items.find((item) => item.name === profileName)?.uid;
    assert.ok(
      profileUid,
      'The profile created through the UI was not persisted.',
    );

    await invoke('save_profile_file', { uid: profileUid, fileData: fixture });
    await invoke('activate_profile', { uid: null });

    const profileCard = await $(
      `//*[normalize-space()="${profileName}"]/ancestor::div[contains(@class,"cursor-pointer")][1]`,
    );
    await profileCard.click();
    await browser.waitUntil(
      async () =>
        (await invoke<ProfilesResponse>('get_profiles')).current === profileUid,
      {
        timeout: 30_000,
        timeoutMsg: 'The local profile was not activated through the UI.',
      },
    );

    const proxiesLink = await $('//*[normalize-space()="代理"]');
    await proxiesLink.waitForClickable();
    await proxiesLink.click();
    await waitForPath('/proxies');

    await browser.waitUntil(
      async () =>
        browser.execute(() => {
          const text = document.body.innerText;
          return text.includes('Proxy Groups') || text.includes('代理集');
        }),
      { timeout: 30_000, timeoutMsg: 'The proxy page did not render.' },
    );

    const evidencePath = process.env.CHIMERA_E2E_EVIDENCE_PATH;
    if (evidencePath) {
      fs.mkdirSync(path.dirname(evidencePath), { recursive: true });
      await browser.saveScreenshot(evidencePath);
    }

    const state = await browser.execute(() => ({
      bodyText: document.body.innerText,
    }));

    assert.equal(state.bodyText.includes('Proxy Groups'), false);
    assert.equal(state.bodyText.includes('Rule'), false);
    assert.equal(state.bodyText.includes('Global'), false);
    assert.equal(state.bodyText.includes('Direct'), false);
    assert.equal(state.bodyText.includes('代理集'), true);
    assert.equal(state.bodyText.includes('规则'), true);
    assert.equal(state.bodyText.includes('全局'), true);
    assert.equal(state.bodyText.includes('直连'), true);
  });
});
