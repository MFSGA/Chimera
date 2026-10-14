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

const profileName = `TDD Main Proxy Node ${Date.now()}`;
const groupName = 'TDD Node Group';
const nodeName = 'TDD SOCKS Node';
type ProxiesResponse = {
  groups: Array<{ name: string; all: Array<{ name: string }> }>;
};
const fixture = `mixed-port: 27892
allow-lan: false
mode: rule
log-level: silent
tun:
  enable: false
proxies:
  - name: ${nodeName}
    type: socks5
    server: 127.0.0.1
    port: 65535
    udp: true
proxy-groups:
  - name: ${groupName}
    type: select
    proxies:
      - ${nodeName}
      - DIRECT
rules:
  - MATCH,${groupName}
`;

describe('main proxy node reference layout', () => {
  let profileUid: string | undefined;
  let previousCurrent: string | null = null;
  let selectionMayHaveChanged = false;

  before(async () => {
    await browser.setWindowSize(1240, 638);
    await openMainRoute('/main/proxies');
    await browser.setWindowSize(1240, 638);

    const initial = await readProfiles();
    previousCurrent = initial.current ?? null;
    const created = await createProfile(
      localConfigProfileRequest(profileName),
      fixture,
    );
    profileUid = created.value;
    committedValue(created, 'local proxy-node Profile creation');
    const profiles = await readProfiles();
    profileUid ??= profiles.items.find(
      (item) => item.name === profileName,
    )?.uid;
    assert.ok(profileUid, 'The isolated proxy node profile was not created.');

    selectionMayHaveChanged = true;
    appliedValue(
      await invoke('activate_profile', { uid: profileUid }),
      'proxy-node Profile activation',
    );

    await browser.waitUntil(
      async () => {
        const proxies = await invoke<ProxiesResponse>('get_proxies').catch(
          () => undefined,
        );
        return proxies?.groups.some(
          (group) =>
            group.name === groupName &&
            group.all.some((node) => node.name === nodeName),
        );
      },
      {
        timeout: 30_000,
        timeoutMsg: 'The active Profile did not expose its proxy group.',
      },
    );

    await browser.refresh();
    await openMainRoute('/main/proxies');
    await browser.setWindowSize(1240, 638);

    const proxiesLink = await $('a[href^="/main/proxies/"]');
    await proxiesLink.waitForExist({ timeout: 30_000 });
    const proxiesPath = await proxiesLink.getAttribute('href');
    assert.ok(proxiesPath, 'The proxy group route was not available.');
    await openMainRoute(proxiesPath);
    await browser.waitUntil(
      async () =>
        browser.execute(
          (expected) => document.body.innerText.includes(expected),
          groupName,
        ),
      {
        timeout: 30_000,
        timeoutMsg: 'The proxy group detail did not render.',
      },
    );
  });

  after(async () => {
    await runCleanupActions('main proxy-node fixture', [
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

  // Contract: an active local Profile provides the SOCKS5 fixture. The proxy
  // index reloads after activation so its group query is populated before the
  // detail route mounts the virtualized node list. Its type and UDP chips must
  // render on the first viewport attachment, catching a virtualizer initialized
  // while its parent viewport ref is still null.
  it('shows ref-style type and UDP chips on proxy nodes', async () => {
    const node = await $(`[data-slot="proxies-virtual-item"]*=${nodeName}`);
    await node.waitForDisplayed({ timeout: 15_000 });

    const state = (await node.execute((element) => {
      const chips = Array.from(
        element.querySelectorAll<HTMLElement>(
          '[data-slot="proxy-node-feature"]',
        ),
      ).map((chip) => chip.innerText.trim());
      const delay = element.querySelector<HTMLElement>(
        '[data-slot="proxy-node-delay"]',
      );
      const button = element.querySelector<HTMLElement>('button');

      return {
        chips,
        hasDelay: Boolean(delay),
        nodeText: element.textContent ?? '',
        buttonHeight: button
          ? Math.round(button.getBoundingClientRect().height)
          : 0,
      };
    })) as {
      chips: string[];
      hasDelay: boolean;
      nodeText: string;
      buttonHeight: number;
    };

    assert.ok(
      state.chips.some((chip) => chip.toLowerCase().includes('socks')),
      JSON.stringify(state, null, 2),
    );
    assert.ok(state.chips.includes('UDP'), JSON.stringify(state, null, 2));
    assert.equal(state.hasDelay, false, JSON.stringify(state, null, 2));
    assert.ok(state.buttonHeight >= 48, JSON.stringify(state, null, 2));

    const evidencePath = process.env.CHIMERA_E2E_EVIDENCE_PATH;
    if (evidencePath) {
      fs.mkdirSync(path.dirname(evidencePath), { recursive: true });
      await browser.saveScreenshot(evidencePath);
    }
  });

  it('uses the main tooltip trigger contract for group delay testing', async () => {
    const trigger = await $('[data-slot="delay-test-button-trigger"]');
    await trigger.waitForDisplayed({ timeout: 15_000 });

    const state = await browser.execute(() => {
      const trigger = document.querySelector<HTMLElement>(
        '[data-slot="delay-test-button-trigger"]',
      );
      return {
        tooltipState: trigger?.getAttribute('data-state') ?? null,
        loadingState: trigger?.getAttribute('data-loading') ?? null,
      };
    });

    assert.equal(state.tooltipState, 'closed', JSON.stringify(state, null, 2));
    assert.equal(state.loadingState, 'false', JSON.stringify(state, null, 2));
  });
});
