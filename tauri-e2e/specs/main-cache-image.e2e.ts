import assert from 'node:assert/strict';
import fs from 'node:fs';
import path from 'node:path';

const profileName = 'TDD Cache Icon Profile';
const groupName = 'TDD Icon Group';
const iconUrl = 'https://example.com/chimera-cache-icon.png';
const fixture = `mixed-port: 27891
allow-lan: false
mode: rule
log-level: silent
tun:
  enable: false
proxies:
  - name: TDD Icon Node
    type: socks5
    server: 127.0.0.1
    port: 65535
proxy-groups:
  - name: ${groupName}
    type: select
    icon: ${iconUrl}
    proxies:
      - TDD Icon Node
      - DIRECT
rules:
  - MATCH,${groupName}
`;

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

async function openMainWindow() {
  await invoke('create_main_window');
  await browser.waitUntil(
    async () => (await browser.getWindowHandles()).includes('main'),
    { timeout: 15_000, timeoutMsg: 'The main window was not created.' },
  );
  await browser.switchToWindow('main');
}

describe('main cached proxy icons', () => {
  let profileUid: string | undefined;

  before(async () => {
    await browser.setWindowSize(1240, 638);

    const existing = await invoke<ProfilesResponse>('get_profiles');
    const stale = existing.items.find((item) => item.name === profileName);
    if (stale) {
      await invoke('activate_profile', { uid: null }).catch(() => undefined);
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
      fileData: fixture,
    });

    const profiles = await invoke<ProfilesResponse>('get_profiles');
    profileUid = profiles.items.find((item) => item.name === profileName)?.uid;
    assert.ok(profileUid, 'The isolated icon profile was not created.');

    await invoke('activate_profile', { uid: profileUid });

    await browser.execute(() => {
      localStorage.setItem(btoa('paraglide-language-cache'), 'zh-cn');
    });
    await openMainWindow();
    await browser.setWindowSize(1240, 638);

    const proxiesLink = await $('a[href^="/main/proxies"]');
    await proxiesLink.waitForClickable({ timeout: 15_000 });
    await proxiesLink.click();
    await browser.waitUntil(
      async () =>
        browser.execute(() => location.pathname.startsWith('/main/proxies')),
      {
        timeout: 15_000,
        timeoutMsg: 'Proxies route did not open.',
      },
    );
  });

  after(async () => {
    if (!profileUid) return;
    await invoke('activate_profile', { uid: null }).catch(() => undefined);
    await invoke('delete_profile', { uid: profileUid }).catch(() => undefined);
  });

  it('routes remote group artwork through the local icon cache', async () => {
    const proxies = await invoke<{
      groups: Array<{ name: string; icon?: string | null }>;
    }>('get_proxies');
    const apiGroup = proxies.groups.find((group) => group.name === groupName);

    assert.equal(apiGroup?.icon, iconUrl, JSON.stringify(proxies, null, 2));

    await browser.waitUntil(
      async () =>
        browser.execute(
          (expected) => document.body.innerText.includes(expected),
          groupName,
        ),
      {
        timeout: 30_000,
        timeoutMsg: 'The proxy group with remote artwork did not render.',
      },
    );

    const serverPort = await invoke<number>('get_server_port');
    assert.ok(serverPort > 0, `Invalid local server port: ${serverPort}`);

    await browser.waitUntil(
      async () =>
        browser.execute(
          (expectedIconUrl) =>
            Array.from(document.images).some(
              (item) =>
                item.src === expectedIconUrl ||
                item.src.includes('/cache/icon?url='),
            ),
          iconUrl,
        ),
      {
        timeout: 15_000,
        timeoutMsg: 'The proxy group artwork did not render an image element.',
      },
    );

    const state = await browser.execute((expectedIconUrl) => {
      const images = Array.from(document.images);
      const image = images.find((item) =>
        item.src.includes('/cache/icon?url='),
      );

      const navigate = document.querySelector<HTMLElement>(
        '[data-slot="proxies-navigate"]',
      );

      return {
        imageSrc: image?.src ?? null,
        allImageSrcs: Array.from(document.images).map((item) => item.src),
        navigateText: navigate?.innerText ?? null,
        expectedEncoded: btoa(expectedIconUrl),
        viewport: { width: innerWidth, height: innerHeight },
      };
    }, iconUrl);

    assert.equal(state.viewport.width, 1224);
    assert.equal(state.viewport.height, 629);
    assert.ok(state.imageSrc, JSON.stringify(state, null, 2));
    assert.match(state.imageSrc, /^http:\/\/localhost:\d+\/cache\/icon\?url=/);
    assert.equal(
      state.imageSrc?.includes(state.expectedEncoded),
      true,
      JSON.stringify(state, null, 2),
    );

    const evidencePath = process.env.CHIMERA_E2E_EVIDENCE_PATH;
    if (evidencePath) {
      fs.mkdirSync(path.dirname(evidencePath), { recursive: true });
      await browser.saveScreenshot(evidencePath);
    }
  });
});
