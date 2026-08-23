import assert from 'node:assert/strict';
import fs from 'node:fs';
import path from 'node:path';

const profileName = 'TDD Main Proxy Node';
const groupName = 'TDD Node Group';
const nodeName = 'TDD SOCKS Node';
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

describe('main proxy node reference layout', () => {
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
    assert.ok(profileUid, 'The isolated proxy node profile was not created.');

    await invoke('activate_profile', { uid: profileUid });

    await browser.execute(() => {
      localStorage.setItem(btoa('paraglide-language-cache'), 'zh-cn');
    });
    await openMainWindow();
    await browser.setWindowSize(1240, 638);

    await browser.execute(() => {
      history.pushState({}, '', '/main/proxies');
      window.dispatchEvent(new PopStateEvent('popstate'));
    });
    await browser.waitUntil(
      async () => browser.execute(() => location.pathname === '/main/proxies'),
      { timeout: 15_000, timeoutMsg: 'The proxies route did not open.' },
    );

    const proxiesLink = await $('a[href^="/main/proxies/"]');
    await proxiesLink.waitForExist({ timeout: 30_000 });
    const proxiesPath = await proxiesLink.getAttribute('href');
    assert.ok(proxiesPath, 'The proxy group route was not available.');
    await browser.execute((target) => {
      history.pushState({}, '', target);
      window.dispatchEvent(new PopStateEvent('popstate'));
    }, proxiesPath);
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
    if (!profileUid) return;
    await invoke('activate_profile', { uid: null }).catch(() => undefined);
    await invoke('delete_profile', { uid: profileUid }).catch(() => undefined);
  });

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
        viewport: { width: innerWidth, height: innerHeight },
      };
    })) as {
      chips: string[];
      hasDelay: boolean;
      nodeText: string;
      buttonHeight: number;
      viewport: { width: number; height: number };
    };

    assert.equal(state.viewport.width, 1224);
    assert.equal(state.viewport.height, 629);
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
