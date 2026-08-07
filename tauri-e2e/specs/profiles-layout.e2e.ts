import assert from 'node:assert/strict';

const targetPath = '/main/profiles/profile';
const draftName = `Main Import Draft ${Date.now()}`;

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

describe('main profiles import action compatibility', () => {
  before(async () => {
    await browser.setWindowSize(1240, 638);
    await browser.execute(() => {
      localStorage.setItem(btoa('paraglide-language-cache'), 'zh-cn');
    });
    await openMainWindow();
    await browser.setWindowSize(1240, 638);

    const link = await $(`a[href="${targetPath}"]`);
    await link.waitForClickable({ timeout: 15_000 });
    await link.click();
    await browser.waitUntil(
      async () =>
        browser.execute(
          (expected) => location.pathname === expected,
          targetPath,
        ),
      { timeout: 15_000, timeoutMsg: 'Profiles route did not open.' },
    );
  });

  it('opens a local import draft from the action search param without persisting on close', async () => {
    const before = await invoke<ProfilesResponse>('get_profiles');
    assert.equal(
      before.items.some((item) => item.name === draftName),
      false,
    );

    const current = new URL(await browser.getUrl());
    current.searchParams.set('action', 'ImportLocalProfile');
    await browser.url(current.href);

    const nameInput = await $('input[name="name"]');
    await nameInput.waitForDisplayed({ timeout: 15_000 });
    await nameInput.setValue(draftName);

    const closeButton = await $('//button[normalize-space()="Close"]');
    await closeButton.waitForClickable({ timeout: 15_000 });
    await closeButton.click();
    await nameInput.waitForDisplayed({ reverse: true, timeout: 15_000 });

    const after = await invoke<ProfilesResponse>('get_profiles');
    assert.equal(
      after.items.some((item) => item.name === draftName),
      false,
    );
    assert.equal(
      await browser.execute(() =>
        new URL(location.href).searchParams.has('action'),
      ),
      false,
    );
  });
});
