import assert from 'node:assert/strict';
import fs from 'node:fs';
import path from 'node:path';

const profileName = 'TDD Rules Icon';
const groupName = 'TDD Square';
const targetPath = '/main/rules';
const rawSvg =
  '<svg xmlns="http://www.w3.org/2000/svg" width="24" height="24" viewBox="0 0 24 24"><rect width="24" height="24" rx="2" fill="#ff4d4f"/></svg>';

const fixture = `mixed-port: 27891
allow-lan: false
mode: rule
log-level: silent
proxies:
  - name: TDD Node
    type: socks5
    server: 127.0.0.1
    port: 65535
proxy-groups:
  - name: ${groupName}
    type: select
    icon: '${rawSvg}'
    proxies:
      - TDD Node
      - DIRECT
rules:
  - MATCH,${groupName}
`;

type ProfilesResponse = {
  current: string | null;
  items: Array<{ uid: string; name: string }>;
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

describe('main rules proxy icon reference behavior', () => {
  let profileUid: string | undefined;

  before(async () => {
    await browser.setWindowSize(1240, 638);
    await browser.execute(() => {
      localStorage.setItem(btoa('paraglide-language-cache'), 'zh-cn');
    });

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
      fileData: fixture,
    });

    const profiles = await invoke<ProfilesResponse>('get_profiles');
    profileUid = profiles.items.find((item) => item.name === profileName)?.uid;
    assert.ok(profileUid, 'The isolated proxy-icon profile was not created.');

    if (profiles.current !== profileUid) {
      await invoke('activate_profile', { uid: profileUid });
    }

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
      { timeout: 15_000, timeoutMsg: 'Rules route did not open.' },
    );
  });

  after(async () => {
    if (!profileUid) return;
    await invoke('activate_profile', { uid: null }).catch(() => undefined);
    await invoke('delete_profile', { uid: profileUid }).catch(() => undefined);
  });

  it('renders raw SVG proxy-group icons without forcing the loaded image round', async () => {
    await browser.waitUntil(
      async () =>
        browser.execute((expectedGroup) => {
          const sidebar = document.querySelector<HTMLElement>(
            '[data-slot="slider-sidebar"]',
          );
          const label = Array.from(
            sidebar?.querySelectorAll<HTMLElement>('*') ?? [],
          ).find((element) => element.textContent?.trim() === expectedGroup);
          const image = label
            ?.closest('a')
            ?.querySelector<HTMLImageElement>('img');
          return Boolean(image && image.complete && image.naturalWidth > 0);
        }, groupName),
      { timeout: 30_000, timeoutMsg: 'Rules proxy-group icon did not load.' },
    );

    const state = await browser.execute((expectedGroup) => {
      const sidebar = document.querySelector<HTMLElement>(
        '[data-slot="slider-sidebar"]',
      );
      const label = Array.from(
        sidebar?.querySelectorAll<HTMLElement>('*') ?? [],
      ).find((element) => element.textContent?.trim() === expectedGroup);
      const item = label?.closest('a');
      const image = item?.querySelector<HTMLImageElement>('img');

      return {
        itemFound: Boolean(item),
        imageFound: Boolean(image),
        imageSrc: image?.src ?? '',
        imageClass: image?.className ?? '',
        naturalWidth: image?.naturalWidth ?? 0,
        naturalHeight: image?.naturalHeight ?? 0,
      };
    }, groupName);

    const evidencePath = process.env.CHIMERA_E2E_EVIDENCE_PATH;
    if (evidencePath) {
      fs.mkdirSync(path.dirname(evidencePath), { recursive: true });
      await browser.saveScreenshot(evidencePath);
    }

    assert.equal(state.itemFound, true, JSON.stringify(state, null, 2));
    assert.equal(state.imageFound, true, JSON.stringify(state, null, 2));
    assert.ok(state.naturalWidth > 0, JSON.stringify(state, null, 2));
    assert.ok(state.naturalHeight > 0, JSON.stringify(state, null, 2));
    assert.ok(
      state.imageSrc.startsWith('data:image/svg+xml;base64,'),
      JSON.stringify(state, null, 2),
    );
    assert.equal(
      state.imageClass.includes('rounded-full'),
      false,
      JSON.stringify(state, null, 2),
    );
  });
});
