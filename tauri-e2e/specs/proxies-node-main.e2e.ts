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
type DelayTriggerState = {
  tooltipState: string | null;
  loadingState: string | null;
  visible: boolean;
  rect: { top: number; right: number; bottom: number; left: number };
  viewport: { width: number; height: number };
  hitTarget: boolean;
  hitFailures: number;
  blockers: string[];
  disabled: boolean;
  ariaDisabled: string | null;
  display: string;
  visibility: string;
  opacity: number;
};
type ProxyNodeRenderState = {
  candidateCount: number;
  scrollViewportFound: boolean;
  nodeVisible: boolean;
  chips: string[];
  hasDelay: boolean;
  nodeText: string;
  buttonFound: boolean;
  buttonHeight: number;
  buttonWidth: number;
  buttonInViewport: boolean;
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

async function readDelayTriggerState(): Promise<DelayTriggerState | null> {
  return browser.execute(() => {
    const trigger = document.querySelector<HTMLButtonElement>(
      '[data-slot="delay-test-button-trigger"]',
    );
    if (!trigger) return null;

    const rect = trigger.getBoundingClientRect();
    const style = window.getComputedStyle(trigger);
    const blockers: string[] = [];
    if (style.visibility !== 'visible') {
      blockers.push(`trigger has visibility:${style.visibility}`);
    }
    const withinViewport =
      rect.width > 0 &&
      rect.height > 0 &&
      rect.left >= 0 &&
      rect.top >= 0 &&
      rect.right <= window.innerWidth &&
      rect.bottom <= window.innerHeight;
    if (!withinViewport) blockers.push('outside viewport');

    for (
      let ancestor: HTMLElement | null = trigger;
      ancestor;
      ancestor = ancestor.parentElement
    ) {
      const ancestorStyle = window.getComputedStyle(ancestor);
      const label = `${ancestor.tagName.toLowerCase()}${ancestor.id ? `#${ancestor.id}` : ''}`;
      if (ancestorStyle.display === 'none') {
        blockers.push(`${label} has display:none`);
      }
      if (Number(ancestorStyle.opacity) <= 0) {
        blockers.push(`${label} has opacity:0`);
      }
      if (ancestorStyle.contentVisibility === 'hidden') {
        blockers.push(`${label} has content-visibility:hidden`);
      }

      if (ancestor === trigger) continue;

      const ancestorRect = ancestor.getBoundingClientRect();
      const scaleX = ancestor.offsetWidth
        ? ancestorRect.width / ancestor.offsetWidth
        : 1;
      const scaleY = ancestor.offsetHeight
        ? ancestorRect.height / ancestor.offsetHeight
        : 1;
      const clipLeft = ancestorRect.left + ancestor.clientLeft * scaleX;
      const clipTop = ancestorRect.top + ancestor.clientTop * scaleY;
      const clipRight = clipLeft + ancestor.clientWidth * scaleX;
      const clipBottom = clipTop + ancestor.clientHeight * scaleY;
      if (
        ['hidden', 'clip', 'scroll', 'auto', 'overlay'].includes(
          ancestorStyle.overflowX,
        ) &&
        (rect.left < clipLeft || rect.right > clipRight)
      ) {
        blockers.push(`${label} clips the trigger horizontally`);
      }
      if (
        ['hidden', 'clip', 'scroll', 'auto', 'overlay'].includes(
          ancestorStyle.overflowY,
        ) &&
        (rect.top < clipTop || rect.bottom > clipBottom)
      ) {
        blockers.push(`${label} clips the trigger vertically`);
      }
    }

    let hitFailures = 0;
    for (const xPart of [0.35, 0.5, 0.65]) {
      for (const yPart of [0.35, 0.5, 0.65]) {
        const hit = document.elementFromPoint(
          rect.left + rect.width * xPart,
          rect.top + rect.height * yPart,
        );
        if (!hit || (hit !== trigger && !trigger.contains(hit))) {
          hitFailures += 1;
        }
      }
    }
    const visible = blockers.length === 0;

    return {
      tooltipState: trigger.getAttribute('data-state'),
      loadingState: trigger.getAttribute('data-loading'),
      visible,
      rect: {
        top: rect.top,
        right: rect.right,
        bottom: rect.bottom,
        left: rect.left,
      },
      viewport: { width: window.innerWidth, height: window.innerHeight },
      hitTarget: hitFailures === 0,
      hitFailures,
      blockers,
      disabled: trigger.disabled,
      ariaDisabled: trigger.getAttribute('aria-disabled'),
      display: style.display,
      visibility: style.visibility,
      opacity: Number(style.opacity),
    };
  });
}

async function readProxyNodeRenderState(): Promise<ProxyNodeRenderState> {
  return browser.execute((expectedName) => {
    const getVisibleRect = (
      element: HTMLElement,
      scrollViewport: HTMLElement | null,
    ) => {
      const rect = element.getBoundingClientRect();
      const viewportRect = scrollViewport?.getBoundingClientRect();
      let left = Math.max(rect.left, 0, viewportRect?.left ?? 0);
      let top = Math.max(rect.top, 0, viewportRect?.top ?? 0);
      let right = Math.min(
        rect.right,
        window.innerWidth,
        viewportRect?.right ?? window.innerWidth,
      );
      let bottom = Math.min(
        rect.bottom,
        window.innerHeight,
        viewportRect?.bottom ?? window.innerHeight,
      );

      for (
        let ancestor: HTMLElement | null = element;
        ancestor;
        ancestor = ancestor.parentElement
      ) {
        const style = window.getComputedStyle(ancestor);
        if (
          style.display === 'none' ||
          style.visibility === 'hidden' ||
          style.visibility === 'collapse' ||
          Number(style.opacity) <= 0
        ) {
          return null;
        }

        const bounds = ancestor.getBoundingClientRect();
        if (/^(auto|scroll|hidden|clip|overlay)$/.test(style.overflowX)) {
          left = Math.max(left, bounds.left);
          right = Math.min(right, bounds.right);
        }
        if (/^(auto|scroll|hidden|clip|overlay)$/.test(style.overflowY)) {
          top = Math.max(top, bounds.top);
          bottom = Math.min(bottom, bounds.bottom);
        }
      }

      return {
        width: Math.max(0, right - left),
        height: Math.max(0, bottom - top),
      };
    };
    const scrollViewport = document.querySelector<HTMLElement>(
      '[data-slot="proxies-content-scroll-area"] [data-slot="scroll-area-viewport"]',
    );
    const candidates = Array.from(
      document.querySelectorAll<HTMLElement>(
        '[data-slot="proxies-virtual-item"]',
      ),
    ).filter((element) => element.textContent?.includes(expectedName));
    const node =
      candidates.find((candidate) => {
        const button = candidate.querySelector<HTMLElement>('button');
        const rect = button?.getBoundingClientRect();
        const visibleRect = button && getVisibleRect(button, scrollViewport);
        return Boolean(
          rect &&
          visibleRect &&
          Math.abs(visibleRect.width - rect.width) <= 1 &&
          Math.abs(visibleRect.height - rect.height) <= 1,
        );
      }) ??
      candidates[0] ??
      null;
    const chips = Array.from(
      node?.querySelectorAll<HTMLElement>('[data-slot="proxy-node-feature"]') ??
        [],
    ).map((chip) => chip.innerText.trim());
    const button = node?.querySelector<HTMLButtonElement>('button') ?? null;
    const rect = button?.getBoundingClientRect();
    const visibleRect = button ? getVisibleRect(button, scrollViewport) : null;

    return {
      candidateCount: candidates.length,
      scrollViewportFound: Boolean(scrollViewport),
      nodeVisible: Boolean(
        node &&
        button &&
        visibleRect &&
        visibleRect.width > 0 &&
        visibleRect.height > 0,
      ),
      chips,
      hasDelay: Boolean(node?.querySelector('[data-slot="proxy-node-delay"]')),
      nodeText: node?.textContent ?? '',
      buttonFound: Boolean(button),
      buttonHeight: rect?.height ?? 0,
      buttonWidth: rect?.width ?? 0,
      buttonInViewport: Boolean(
        rect &&
        visibleRect &&
        visibleRect.width === rect.width &&
        visibleRect.height === rect.height,
      ),
    };
  }, nodeName);
}

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
  // detail route mounts the virtualized node list. Poll the current visible
  // DOM for a measured button fully inside the content scroll viewport, then
  // verify its type and UDP chips; the bounded wait catches a virtualizer that
  // never attaches to its viewport.
  it('shows ref-style type and UDP chips on proxy nodes', async () => {
    let lastState: ProxyNodeRenderState | null = null;
    try {
      await browser.waitUntil(
        async () => {
          lastState = await readProxyNodeRenderState();
          return (
            lastState.nodeVisible &&
            lastState.scrollViewportFound &&
            lastState.buttonFound &&
            lastState.buttonHeight >= 48 &&
            lastState.buttonWidth > 0 &&
            lastState.buttonInViewport
          );
        },
        {
          timeout: 15_000,
          timeoutMsg: 'The proxy node did not reach its visible layout.',
        },
      );
    } catch (cause) {
      throw new Error(
        `The proxy node did not reach its visible layout: ${JSON.stringify(lastState)}`,
        { cause },
      );
    }
    const state = await readProxyNodeRenderState();

    assert.ok(
      state.chips.some((chip) => chip.toLowerCase().includes('socks')),
      JSON.stringify(state, null, 2),
    );
    assert.ok(state.chips.includes('UDP'), JSON.stringify(state, null, 2));
    assert.equal(state.hasDelay, false, JSON.stringify(state, null, 2));
    assert.ok(state.scrollViewportFound, JSON.stringify(state, null, 2));
    assert.ok(state.nodeVisible, JSON.stringify(state, null, 2));
    assert.ok(state.buttonInViewport, JSON.stringify(state, null, 2));
    assert.ok(state.buttonHeight >= 48, JSON.stringify(state, null, 2));

    const evidencePath = process.env.CHIMERA_E2E_EVIDENCE_PATH;
    if (evidencePath) {
      fs.mkdirSync(path.dirname(evidencePath), { recursive: true });
      await browser.saveScreenshot(evidencePath);
    }
  });

  // Contract: the group detail shows the full delay-test action within the
  // viewport and ancestor overflow clips; inset hit-test samples stay on the button.
  // It starts idle with its tooltip closed. The failure artifact for
  // this view showed the action rendered and visible even though WDIO's
  // waitForDisplayed reported it hidden; check rendered geometry and hit
  // testing representative points inside its rounded edge, then assert the
  // action's enabled, idle state.
  it('renders an available, idle group delay trigger', async () => {
    let lastState: DelayTriggerState | null = null;
    try {
      await browser.waitUntil(
        async () => {
          lastState = await readDelayTriggerState();

          return (
            lastState?.visible === true &&
            lastState.hitTarget &&
            !lastState.disabled &&
            lastState.ariaDisabled !== 'true'
          );
        },
        {
          timeout: 15_000,
          timeoutMsg:
            'The group delay trigger did not become visible and available.',
        },
      );
    } catch (error) {
      throw new Error(
        `The group delay trigger did not become visible and available: ${JSON.stringify(lastState)}`,
        { cause: error },
      );
    }

    const state = await readDelayTriggerState();
    assert.ok(state, 'The group delay trigger was not rendered.');
    assert.equal(state.visible, true, JSON.stringify(state, null, 2));
    assert.equal(state.hitTarget, true, JSON.stringify(state, null, 2));
    assert.equal(state.disabled, false, JSON.stringify(state, null, 2));
    assert.notEqual(state.ariaDisabled, 'true', JSON.stringify(state, null, 2));
    assert.equal(state.tooltipState, 'closed', JSON.stringify(state, null, 2));
    assert.equal(state.loadingState, 'false', JSON.stringify(state, null, 2));
  });
});
