import assert from 'node:assert/strict';
import fs from 'node:fs';
import path from 'node:path';
import { openMainRoute } from './main-window.js';
import {
  appliedValue,
  committedValue,
  createProfile,
  invoke,
  localConfigProfileRequest,
  readProfiles,
  runCleanupActions,
} from './profile-fixtures.js';

const profileName = `TDD Cache Icon Profile ${Date.now()}`;
const groupName = 'TDD Icon Group';
const iconUrl = 'https://example.com/chimera-cache-icon.png';
const svgGroupName = 'TDD SVG Group';
const rawSvg =
  '<svg xmlns="http://www.w3.org/2000/svg" width="24" height="24" viewBox="0 0 24 24"><rect width="24" height="24" rx="2" fill="#ff4d4f"/></svg>';
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
  - name: ${svgGroupName}
    type: select
    icon: '${rawSvg}'
    proxies:
      - TDD Icon Node
      - DIRECT
rules:
  - DOMAIN-SUFFIX,example.com,${svgGroupName}
  - MATCH,${groupName}
`;

describe('main cached proxy icons', () => {
  let profileUid: string | undefined;
  let previousCurrent: string | null = null;
  let selectionMayHaveChanged = false;

  before(async () => {
    await browser.setWindowSize(1240, 638);
    const initial = await readProfiles();
    previousCurrent = initial.current ?? null;
    const created = await createProfile(
      localConfigProfileRequest(profileName),
      fixture,
    );
    profileUid = created.value;
    committedValue(created, 'local icon Profile creation');

    const profiles = await readProfiles();
    profileUid ??= profiles.items.find(
      (item) => item.name === profileName,
    )?.uid;
    assert.ok(profileUid, 'The isolated icon profile was not created.');

    selectionMayHaveChanged = true;
    appliedValue(
      await invoke('activate_profile', { uid: profileUid }),
      'icon Profile activation',
    );

    await browser.execute(() => {
      localStorage.setItem(btoa('paraglide-language-cache'), 'zh-cn');
    });
    await openMainRoute('/main/proxies');
    await browser.setWindowSize(1240, 638);
  });

  after(async () => {
    await runCleanupActions('main cached proxy icon fixture', [
      {
        label: 'restore Profile selection',
        run: async () => {
          if (!selectionMayHaveChanged) return;
          committedValue(
            await invoke('activate_profile', { uid: previousCurrent }),
            'Profile selection restoration',
          );
        },
      },
      {
        label: 'delete test Profile',
        run: async () => {
          const uid =
            profileUid ??
            (await readProfiles()).items.find(
              (item) => item.name === profileName,
            )?.uid;
          if (!uid) return;
          committedValue(
            await invoke('delete_profile', { uid }),
            'Profile deletion',
          );
        },
      },
    ]);
  });

  // Contract: after IPC setup creates and activates this run's local Profile,
  // the real proxy API exposes its remote icon URL and the main UI renders the
  // proxy group artwork via the local cache route. API data and image loading
  // are independent evidence; a stale create-profile wire fails fixture setup.
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

    assert.ok(state.viewport.width >= 1200, JSON.stringify(state, null, 2));
    assert.ok(state.viewport.height >= 600, JSON.stringify(state, null, 2));
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

  it('keeps loaded Rules proxy artwork square like ref', async () => {
    await openMainRoute('/main/rules');

    let lastState = 'not observed';
    const readState = () =>
      browser.execute((expectedGroup) => {
        const sidebar = document.querySelector<HTMLElement>(
          '[data-slot="slider-sidebar"]',
        );
        const link = Array.from(
          sidebar?.querySelectorAll<HTMLAnchorElement>('a') ?? [],
        ).find((candidate) => candidate.textContent?.trim() === expectedGroup);
        const image = link?.querySelector<HTMLImageElement>('img');

        return {
          imageFound: Boolean(image),
          imageSrc: image?.src ?? '',
          imageClass: image?.className ?? '',
          width: image?.getBoundingClientRect().width ?? 0,
          height: image?.getBoundingClientRect().height ?? 0,
        };
      }, svgGroupName);

    try {
      await browser.waitUntil(
        async () => {
          const state = await readState();
          lastState = JSON.stringify(state);
          return state.imageFound && state.width > 0 && state.height > 0;
        },
        {
          timeout: 30_000,
          interval: 250,
          timeoutMsg: 'Rules proxy artwork did not load in the sidebar.',
        },
      );
    } catch (cause) {
      throw new Error(
        `Rules proxy artwork did not load in the sidebar. Last state: ${lastState}`,
        { cause },
      );
    }

    const state = await readState();

    assert.equal(state.imageFound, true, JSON.stringify(state, null, 2));
    assert.ok(
      state.imageSrc.startsWith('data:image/svg+xml;base64,'),
      JSON.stringify(state, null, 2),
    );
    assert.equal(
      state.imageClass.includes('rounded-full'),
      false,
      JSON.stringify(state, null, 2),
    );
    assert.ok(
      Math.abs(state.width - 24) < 0.01,
      JSON.stringify(state, null, 2),
    );
    assert.ok(
      Math.abs(state.height - 24) < 0.01,
      JSON.stringify(state, null, 2),
    );
  });
});
