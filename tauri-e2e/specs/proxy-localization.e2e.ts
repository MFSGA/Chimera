import assert from 'node:assert/strict';
import fs from 'node:fs';
import path from 'node:path';
import { fileURLToPath } from 'node:url';
import type { MutationOutcome } from '../../frontend/interface/src/ipc/bindings.js';
import { committedValue } from './profile-fixtures.js';

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

type VergeConfig = {
  language?: string | null;
};

type WebviewLocaleState = {
  cache: string | null;
  documentLanguage: string;
};

async function attemptCleanup(
  errors: Error[],
  label: string,
  action: () => Promise<void>,
) {
  try {
    await action();
  } catch (cause) {
    errors.push(new Error(label, { cause }));
  }
}

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
  // Contract (desktop UI E2E): set the test-owned app language to zh-cn, create
  // a local Profile through the legacy UI, attach its fixture through IPC, and
  // activate it through the UI. The independent oracle is the page title and
  // static mode-control labels; dynamic proxy/node names can legitimately be
  // English. Restore the original language, selected Profile, and each
  // WebView's cached locale, then delete only the Profile created by this test.
  let profileUid: string | undefined;
  let originalLanguage: string | null | undefined;
  let originalCurrentProfile: string | null = null;
  const originalWebviewLocales = new Map<string, WebviewLocaleState>();

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

    const activeWindow = await browser.getWindowHandle();
    for (const handle of await browser.getWindowHandles()) {
      if (handle !== 'legacy' && handle !== 'main') continue;
      await browser.switchToWindow(handle);
      originalWebviewLocales.set(
        handle,
        await browser.execute(() => ({
          cache: localStorage.getItem(btoa('paraglide-language-cache')),
          documentLanguage: document.documentElement.lang,
        })),
      );
    }
    await browser.switchToWindow(activeWindow);

    const originalConfig = await invoke<VergeConfig>('get_verge_config');
    originalLanguage = originalConfig.language ?? null;
    const originalProfiles = await invoke<ProfilesResponse>('get_profiles');
    assert.equal(
      originalProfiles.items.some((item) => item.name === profileName),
      false,
      'The isolated E2E runtime already contains this test profile.',
    );
    originalCurrentProfile = originalProfiles.current;
    await invoke('patch_verge_config', {
      payload: { language: 'zh-cn' },
    });
    await browser.waitUntil(
      async () =>
        (
          await invoke<VergeConfig>('get_verge_config')
        ).language?.toLowerCase() === 'zh-cn',
      {
        timeout: 15_000,
        timeoutMsg: 'The configured language did not switch to zh-cn.',
      },
    );
    await browser.refresh();

    const currentUrl = new URL(await browser.getUrl());
    currentUrl.pathname = '/profiles';
    currentUrl.search = '';
    await browser.url(currentUrl.href);
    await waitForPath('/profiles');
  });

  after(async () => {
    const cleanupErrors: Error[] = [];
    const profiles = await invoke<ProfilesResponse>('get_profiles').catch(
      (cause: unknown) => {
        cleanupErrors.push(
          new Error('Could not find the test profile for cleanup.', { cause }),
        );
        return null;
      },
    );
    profileUid ??= profiles?.items.find(
      (item) => item.name === profileName,
    )?.uid;

    if (profileUid) {
      await attemptCleanup(
        cleanupErrors,
        'Could not restore the active profile.',
        async () => {
          committedValue(
            await invoke<MutationOutcome<null>>('activate_profile', {
              uid: originalCurrentProfile,
            }),
            'Profile selection restoration',
          );
          await browser.waitUntil(
            async () =>
              (await invoke<ProfilesResponse>('get_profiles')).current ===
              originalCurrentProfile,
            {
              timeout: 30_000,
              timeoutMsg: 'The original profile selection was not restored.',
            },
          );
        },
      );
      await attemptCleanup(
        cleanupErrors,
        'Could not delete the test profile.',
        async () => {
          committedValue(
            await invoke<MutationOutcome<null>>('delete_profile', {
              uid: profileUid,
            }),
            'Test profile deletion',
          );
          await browser.waitUntil(
            async () =>
              !(await invoke<ProfilesResponse>('get_profiles')).items.some(
                (item) => item.uid === profileUid,
              ),
            {
              timeout: 30_000,
              timeoutMsg: 'The test profile remained after deletion.',
            },
          );
        },
      );
    }

    if (originalLanguage !== undefined) {
      await attemptCleanup(
        cleanupErrors,
        'Could not restore the original language.',
        async () => {
          await invoke('patch_verge_config', {
            payload: { language: originalLanguage ?? null },
          });
          await browser.waitUntil(
            async () =>
              ((await invoke<VergeConfig>('get_verge_config')).language ??
                null) === (originalLanguage ?? null),
            {
              timeout: 15_000,
              timeoutMsg: 'The original language setting was not restored.',
            },
          );
        },
      );
    }

    for (const [handle, localeState] of originalWebviewLocales) {
      await attemptCleanup(
        cleanupErrors,
        `Could not restore the original locale in ${handle}.`,
        async () => {
          if (!(await browser.getWindowHandles()).includes(handle)) return;
          await browser.switchToWindow(handle);
          await browser.execute((state) => {
            const key = btoa('paraglide-language-cache');
            if (state.cache === null) {
              localStorage.removeItem(key);
            } else {
              localStorage.setItem(key, state.cache);
            }
          }, localeState);
          await browser.refresh();
          await browser.waitUntil(
            async () =>
              browser.execute(
                (language) => document.documentElement.lang === language,
                localeState.documentLanguage,
              ),
            {
              timeout: 15_000,
              timeoutMsg: `${handle} did not return to its original locale.`,
            },
          );
          await browser.execute((state) => {
            const key = btoa('paraglide-language-cache');
            if (state.cache === null) {
              localStorage.removeItem(key);
            } else {
              localStorage.setItem(key, state.cache);
            }
          }, localeState);
        },
      );
    }

    if ((await browser.getWindowHandles()).includes('legacy')) {
      await browser.switchToWindow('legacy');
    }

    if (cleanupErrors.length > 0) {
      throw new AggregateError(
        cleanupErrors,
        'Legacy proxy localization cleanup was incomplete.',
      );
    }
  });

  it('localizes the proxy page after creating and activating a local profile', async () => {
    const addButton = await $('button.MuiFab-primary');
    await addButton.waitForClickable();
    await addButton.click();

    const typeSelect = await $('[role="combobox"]');
    await typeSelect.waitForDisplayed({ timeout: 15_000 });
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

    const currentUrl = new URL(await browser.getUrl());
    currentUrl.pathname = '/proxies';
    currentUrl.search = '';
    await browser.url(currentUrl.href);
    await waitForPath('/proxies');

    await browser.waitUntil(
      async () =>
        browser.execute(() => {
          const title = document.querySelector('h1')?.textContent?.trim();
          const modeLabels = Array.from(
            document.querySelectorAll<HTMLButtonElement>(
              '[role="group"] button',
            ),
          ).map((button) => [button.value, button.textContent?.trim()]);
          return (
            title === '代理集' &&
            modeLabels.some(
              ([value, label]) => value === 'rule' && label === '规则',
            ) &&
            modeLabels.some(
              ([value, label]) => value === 'global' && label === '全局',
            ) &&
            modeLabels.some(
              ([value, label]) => value === 'direct' && label === '直连',
            )
          );
        }),
      {
        timeout: 30_000,
        timeoutMsg:
          'The proxy title and mode controls did not localize to zh-cn.',
      },
    );

    const evidencePath = process.env.CHIMERA_E2E_EVIDENCE_PATH;
    if (evidencePath) {
      fs.mkdirSync(path.dirname(evidencePath), { recursive: true });
      await browser.saveScreenshot(evidencePath);
    }

    const state = await browser.execute(() => ({
      title: document.querySelector('h1')?.textContent?.trim(),
      modeLabels: Array.from(
        document.querySelectorAll<HTMLButtonElement>('[role="group"] button'),
      ).map((button) => [button.value, button.textContent?.trim()]),
    }));

    assert.equal(state.title, '代理集');
    assert.ok(
      state.modeLabels.some(
        ([value, label]) => value === 'rule' && label === '规则',
      ),
    );
    assert.ok(
      state.modeLabels.some(
        ([value, label]) => value === 'global' && label === '全局',
      ),
    );
    assert.ok(
      state.modeLabels.some(
        ([value, label]) => value === 'direct' && label === '直连',
      ),
    );
  });
});
