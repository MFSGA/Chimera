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

const profileName = `TDD Rules Icon ${Date.now()}`;
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

type ProxiesResponse = {
  groups: Array<{ name: string; icon?: string | null }>;
};

describe('main rules proxy icon reference behavior', () => {
  let profileUid: string | undefined;
  let previousCurrent: string | null = null;
  let selectionMayHaveChanged = false;

  before(async () => {
    await browser.setWindowSize(1240, 638);
    await browser.execute(() => {
      localStorage.setItem(btoa('paraglide-language-cache'), 'zh-cn');
    });

    const initial = await readProfiles();
    previousCurrent = initial.current ?? null;
    const created = await createProfile(
      localConfigProfileRequest(profileName),
      fixture,
    );
    profileUid = created.value;
    committedValue(created, 'local rules-icon Profile creation');
    const profiles = await readProfiles();
    profileUid ??= profiles.items.find(
      (item) => item.name === profileName,
    )?.uid;
    assert.ok(profileUid, 'The isolated proxy-icon profile was not created.');

    selectionMayHaveChanged = true;
    appliedValue(
      await invoke('activate_profile', { uid: profileUid }),
      'rules-icon Profile activation',
    );
    const proxies = await invoke<ProxiesResponse>('get_proxies');
    assert.ok(
      proxies.groups.some(
        (group) => group.name === groupName && group.icon === rawSvg,
      ),
      'The raw-SVG proxy group did not become active in the Clash runtime.',
    );

    await openMainRoute(targetPath);
    await browser.setWindowSize(1240, 638);
  });

  after(async () => {
    await runCleanupActions('main rules proxy-icon fixture', [
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

  // Contract: activating a test-owned typed local Profile makes its raw SVG
  // proxy-group icon visible through the runtime proxy API, then the main Rules
  // UI renders that icon. Runtime response and loaded image state are separate
  // checks; stale Profile IPC or icon presentation fails the setup or test.
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
