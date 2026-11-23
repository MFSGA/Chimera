import assert from 'node:assert/strict';
import fs from 'node:fs';
import path from 'node:path';
import { readAppMessageCatalog, xpathLiteral } from './app-message-catalog.js';
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
          visibleRect.width > 0 &&
          visibleRect.height > 0 &&
          visibleRect.width === rect.width &&
          visibleRect.height === rect.height,
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
    // Creating the first Config auto-activates it (ref facade semantics).
    // Register recovery before creation, not after a redundant activation.
    selectionMayHaveChanged = true;
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

    if (profiles.current === profileUid) {
      // Ref automatically selects the first Config. When Core was stopped,
      // both creation receipts can correctly be `unchanged`; reselecting
      // this same UID would be another document-only commit, not an apply.
      const applied = created.commits.some(
        (receipt) =>
          receipt.domain === 'profiles' && receipt.runtime === 'applied',
      );
      if (!applied) {
        assert.ok(
          created.commits.every(
            (receipt) =>
              receipt.runtime === 'unchanged' ||
              receipt.runtime === 'saved_inactive',
          ),
          `Unexpected automatic Profile activation receipts: ${JSON.stringify(created.commits)}`,
        );
        // Test preparation only: make the selected Config actually run via
        // the production lifecycle. Do not fake an applied receipt.
        await invoke('restart_sidecar');
      }
    } else {
      appliedValue(
        await invoke('activate_profile', { uid: profileUid }),
        'proxy-node Profile activation',
      );
    }

    let lastCoreState: unknown;
    try {
      await browser.waitUntil(
        async () => {
          const [state] =
            await invoke<[unknown, number, string]>('get_core_status');
          lastCoreState = state;
          return state === 'Running';
        },
        {
          timeout: 30_000,
          timeoutMsg: 'The synthetic Profile Core did not start.',
        },
      );
    } catch (error) {
      throw new Error(
        `Synthetic Profile Core is not running: ${JSON.stringify(lastCoreState)}`,
        { cause: error },
      );
    }

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
    assert.match(
      proxiesPath,
      /^\/main\/proxies\/group\/[^/?]+/,
      'The visible group link must use the canonical ref route.',
    );
    // Navigate through the actual sidebar link, not a test-only URL jump.
    await proxiesLink.click();
    await browser.waitUntil(
      async () => new URL(await browser.getUrl()).pathname === proxiesPath,
      {
        timeout: 15_000,
        timeoutMsg: 'The visible group link did not navigate.',
      },
    );
    // The href is already canonical. This does not navigate again; it waits
    // for the nested animated outlet to finish before any hit-testing/hover.
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

    // Ref GroupMeta mounts a hidden measuring copy before the visible row.
    // Assert the real, user-visible status rather than the aria-hidden clone.
    const groupStatus = await $(
      '[data-slot="proxies-group-meta"] > div:not([aria-hidden]) [data-slot="proxies-group-status"]',
    );
    await groupStatus.waitForDisplayed({ timeout: 15_000 });
    assert.ok((await groupStatus.getText()).trim().length > 0);
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
  it('shows one shared latency-history tooltip when hovering a node card', async () => {
    const card = await $(
      '[data-slot="proxies-virtual-item"] button[data-selectable]',
    );
    await card.waitForDisplayed({ timeout: 15_000 });
    // The embedded driver sometimes starts with the pointer over the
    // focused sidebar link after navigation. Make two genuine W3C pointer
    // moves within the card so its React pointermove handler is exercised.
    await browser
      .action('pointer', { parameters: { pointerType: 'mouse' } })
      .move({ origin: card, x: -25, y: 0 })
      .move({ origin: card, x: 0, y: 0 })
      .perform();

    const history = await $('[data-slot="proxy-delay-history"]');
    await history.waitForDisplayed({ timeout: 15_000 });
    assert.ok((await history.getText()).trim().length > 0);
    const visibleTooltips = await $$('[data-slot="proxy-delay-history"]');
    assert.equal(visibleTooltips.length, 1);
  });

  it('selects a proxy through the visible card and restores the previous selection', async () => {
    const readSelected = async () => {
      const selected = await $$(
        '[data-slot="proxies-virtual-item"][data-active="true"] [data-slot="proxy-node-card"]',
      );
      return (await selected.length) > 0
        ? selected[0].getAttribute('data-proxy-name')
        : null;
    };

    const previous = await readSelected();
    assert.ok(
      previous === nodeName || previous === 'DIRECT',
      `Unexpected initial selected proxy: ${previous}`,
    );
    const next = previous === nodeName ? 'DIRECT' : nodeName;
    const nextCard = await $(
      `[data-slot="proxy-node-card"][data-proxy-name="${next}"]`,
    );
    await nextCard.waitForDisplayed({ timeout: 15_000 });
    assert.equal(await nextCard.getAttribute('data-selectable'), 'true');
    assert.equal(await nextCard.getAttribute('aria-disabled'), 'false');

    try {
      await nextCard.click();
      await browser.waitUntil(async () => (await readSelected()) === next, {
        timeout: 15_000,
        timeoutMsg: `Clicking ${next} did not change the selected proxy.`,
      });
    } finally {
      const previousCard = await $(
        `[data-slot="proxy-node-card"][data-proxy-name="${previous}"]`,
      );
      await previousCard.waitForDisplayed({ timeout: 15_000 });
      await previousCard.click();
      await browser.waitUntil(async () => (await readSelected()) === previous, {
        timeout: 15_000,
        timeoutMsg: 'The original selected proxy was not restored.',
      });
    }
  });

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

  it('completes a real group delay action with visible success or localized error', async () => {
    const catalog = await readAppMessageCatalog();
    const failureText = catalog.error_clash_read_delay_test_failed;
    assert.equal(typeof failureText, 'string');

    const groupAction = await $('[data-slot="delay-test-button"]');
    const groupTrigger = await $('[data-slot="delay-test-button-trigger"]');
    await groupTrigger.waitForDisplayed({ timeout: 15_000 });
    assert.equal(await groupAction.getAttribute('data-loading'), 'false');
    assert.equal(await groupAction.getAttribute('data-completed'), 'false');
    assert.equal(await groupAction.getAttribute('data-error'), 'false');

    // This invokes the live Core's native group delay action through the
    // visible UI in an isolated namespace; no injected success or fake sample.
    await groupTrigger.click();
    await browser.waitUntil(
      async () =>
        (await groupAction.getAttribute('data-completed')) === 'true' ||
        (await groupAction.getAttribute('data-error')) === 'true',
      {
        timeout: 30_000,
        timeoutMsg:
          'Group delay action did not report a completed test or an error.',
      },
    );
    assert.equal(await groupAction.getAttribute('data-loading'), 'false');
    const errored = (await groupAction.getAttribute('data-error')) === 'true';
    console.info('GROUP_DELAY_UI_RESULT', errored ? 'error' : 'completed');
    if (errored) {
      assert.equal(await groupAction.getAttribute('data-completed'), 'false');
      assert.equal(await groupAction.getAttribute('data-success'), 'false');
      await browser.waitUntil(
        async () =>
          browser.execute(
            (message) => document.body.innerText.includes(message),
            failureText as string,
          ),
        {
          timeout: 5_000,
          timeoutMsg: 'A rejected group delay had no localized visible error.',
        },
      );
      // Avoid mistaking this group error for the next node action's notice.
      await browser.waitUntil(
        async () =>
          browser.execute(
            (message) => !document.body.innerText.includes(message),
            failureText as string,
          ),
        {
          timeout: 10_000,
          timeoutMsg: 'The group failure notice did not close.',
        },
      );
    } else {
      assert.equal(await groupAction.getAttribute('data-completed'), 'true');
    }
  });

  it('tests a real offline node without selecting it and reports the result', async () => {
    const catalog = await readAppMessageCatalog();
    const failureText = catalog.error_clash_read_delay_test_failed;
    assert.equal(typeof failureText, 'string');

    const activeBefore = await $(
      '[data-slot="proxies-virtual-item"][data-active="true"] [data-slot="proxy-node-card"]',
    );
    const nameBefore = await activeBefore.getAttribute('data-proxy-name');
    const card = await $(
      `[data-slot="proxy-node-card"][data-proxy-name="${nodeName}"]`,
    );
    const delayTrigger = await card.$(
      '[data-slot="proxy-node-delay-test-trigger"]',
    );
    await delayTrigger.waitForDisplayed({ timeout: 15_000 });
    assert.equal(await delayTrigger.getAttribute('data-error'), 'false');

    // Prove that the nested delay action does not select a *different* node.
    // Otherwise clicking the already-selected SOCKS node could hide a bubble
    // into its parent card's select handler.
    if (nameBefore === nodeName) {
      const directCard = await $(
        '[data-slot="proxy-node-card"][data-proxy-name="DIRECT"]',
      );
      await directCard.click();
      await browser.waitUntil(
        async () =>
          (await $(
            '[data-slot="proxies-virtual-item"][data-active="true"] [data-slot="proxy-node-card"]',
          ).getAttribute('data-proxy-name')) === 'DIRECT',
        {
          timeout: 15_000,
          timeoutMsg:
            'Could not select DIRECT before testing nested delay click.',
        },
      );
    }

    try {
      // The fixture's SOCKS5 port is deliberately closed in this isolated
      // network, so the native probe cannot reach a live external endpoint.
      await delayTrigger.click();
      await browser.waitUntil(
        async () =>
          (await delayTrigger.getAttribute('data-error')) === 'true' ||
          (await delayTrigger.getAttribute('data-success')) === 'true',
        {
          timeout: 30_000,
          timeoutMsg: 'Node delay action remained pending with no result.',
        },
      );
      const nodeErrored =
        (await delayTrigger.getAttribute('data-error')) === 'true';
      console.info('NODE_DELAY_UI_RESULT', nodeErrored ? 'error' : 'completed');
      assert.equal(
        nodeErrored,
        true,
        'The isolated SOCKS5 endpoint is closed; the native node probe must report a rejected request.',
      );
      if (nodeErrored) {
        await browser.waitUntil(
          async () =>
            browser.execute(
              (message) => document.body.innerText.includes(message),
              failureText as string,
            ),
          {
            timeout: 5_000,
            timeoutMsg: 'A rejected node delay had no localized visible error.',
          },
        );
      }

      const activeAfter = await $(
        '[data-slot="proxies-virtual-item"][data-active="true"] [data-slot="proxy-node-card"]',
      );
      assert.equal(
        await activeAfter.getAttribute('data-proxy-name'),
        nameBefore === nodeName ? 'DIRECT' : nameBefore,
        'Clicking the nested delay control should not change the selected proxy.',
      );
    } finally {
      if (nameBefore === nodeName) {
        await card.click();
        await browser.waitUntil(
          async () =>
            (await $(
              '[data-slot="proxies-virtual-item"][data-active="true"] [data-slot="proxy-node-card"]',
            ).getAttribute('data-proxy-name')) === nodeName,
          {
            timeout: 15_000,
            timeoutMsg: 'The original selected node could not be restored.',
          },
        );
      }
    }
  });

  it('filters the native node list through the ref search UI and restores it', async () => {
    const navigation = await $('[data-slot="proxies-navigate"]');
    await navigation.waitForDisplayed({ timeout: 15_000 });
    const activeGroup = await $('a[data-group-name]');
    await activeGroup.waitForExist({ timeout: 15_000 });

    const searchButton = await $('[data-slot="proxies-node-search-button"]');
    await searchButton.waitForDisplayed({ timeout: 15_000 });
    await searchButton.click();

    const field = await $('[data-slot="proxies-node-search-field"]');
    await field.waitForDisplayed({ timeout: 15_000 });
    await field.setValue('nonexistent-proxy-node-for-e2e');

    const empty = await $('[data-slot="proxies-node-no-match"]');
    await empty.waitForDisplayed({ timeout: 15_000 });
    await browser.waitUntil(
      async () =>
        browser.execute(
          () =>
            new URLSearchParams(location.search).get('q') ===
            'nonexistent-proxy-node-for-e2e',
        ),
      { timeout: 15_000, timeoutMsg: 'Proxy search did not update the URL.' },
    );

    const clear = await $('[data-slot="search-field-clear"]');
    await clear.waitForDisplayed({ timeout: 15_000 });
    await clear.click();
    await browser.keys('Escape');
    await browser.waitUntil(
      async () =>
        browser.execute(() => !new URLSearchParams(location.search).has('q')),
      {
        timeout: 15_000,
        timeoutMsg: 'Closed proxy search retained its filter.',
      },
    );
    await browser.waitUntil(
      async () =>
        browser.execute(
          () =>
            document.querySelectorAll('[data-slot="proxies-virtual-item"]')
              .length > 0,
        ),
      {
        timeout: 15_000,
        timeoutMsg: 'Proxy node did not return after search.',
      },
    );

    const sort = await $('[data-slot="proxies-node-sort-button"]');
    await sort.waitForDisplayed({ timeout: 15_000 });
    const locate = await $('[data-slot="proxies-locate-current-node"]');
    await locate.waitForExist({ timeout: 15_000 });
  });

  it('opens a full-width search overlay on narrow screens and closes it visibly', async () => {
    await browser.setWindowSize(420, 740);
    try {
      const open = await $('[data-slot="proxies-node-search-button"]');
      await open.waitForDisplayed({ timeout: 15_000 });
      await open.click();

      const overlay = await $('[data-slot="proxies-node-search-overlay"]');
      await overlay.waitForDisplayed({ timeout: 15_000 });
      const field = await $('[data-slot="proxies-node-search-field"]');
      await field.waitForDisplayed({ timeout: 15_000 });
      await field.setValue('nonexistent-proxy-for-small-screen');
      await $('[data-slot="proxies-node-no-match"]').waitForDisplayed({
        timeout: 15_000,
      });

      const close = await $('[data-slot="proxies-node-search-close"]');
      await close.click();
      await browser.waitUntil(
        async () =>
          !(await overlay.isExisting()) &&
          (await browser.execute(
            () => !new URLSearchParams(location.search).has('q'),
          )),
        {
          timeout: 15_000,
          timeoutMsg: 'Narrow proxy search was not dismissed.',
        },
      );
    } finally {
      await browser.setWindowSize(1240, 638);
    }
  });

  // Acceptance: the ref folder controls must be functional, not merely
  // present. Every mutation below is triggered through a user-visible UI.
  // Read-only get_profiles is used to assert the committed document; cleanup
  // restores only this test Profile's original presentation layout.
  it('creates proxy folders, moves a group, drags it again and restores the layout', async () => {
    assert.ok(profileUid, 'A fixture Config Profile must be selected.');
    const uid = profileUid;
    const before = await readProfiles();
    const original = before.proxy_group_layouts?.[uid];
    const firstName = `E2E Proxy First ${Date.now()}`;
    const secondName = `E2E Proxy Second ${Date.now()}`;

    const layoutForTest = async () =>
      (await readProfiles()).proxy_group_layouts?.[uid];

    const addFolderFromUi = async (name: string) => {
      const add = await $('[data-slot="proxies-folder-add"]');
      await add.waitForClickable({ timeout: 15_000 });
      await add.click();

      const dialog = await $('[data-slot="folder-name-dialog"]');
      await dialog.waitForDisplayed({ timeout: 15_000 });
      const input = await dialog.$('input[aria-label]');
      await input.waitForDisplayed({ timeout: 15_000 });
      await input.setValue(name);
      const save = await dialog.$('button[form="folder-name-dialog-form"]');
      await save.waitForClickable({ timeout: 15_000 });
      await save.click();
      await dialog.waitForDisplayed({ reverse: true, timeout: 15_000 });

      const folder = await $(
        `[data-slot="proxies-folder"][aria-label="${name}"]`,
      );
      await folder.waitForDisplayed({ timeout: 15_000 });
      await browser.waitUntil(
        async () =>
          Boolean(
            (await layoutForTest())?.folders.some((item) => item.name === name),
          ),
        { timeout: 15_000, timeoutMsg: `Folder ${name} was not committed.` },
      );
      return folder;
    };

    try {
      await openMainRoute('/main/proxies');
      await addFolderFromUi(firstName);

      const groupItem = await $(`a[data-group-name="${groupName}"]`);
      await groupItem.waitForDisplayed({ timeout: 15_000 });
      await groupItem.click({ button: 'right' });

      const catalog = await readAppMessageCatalog();
      const moveLabel = catalog.profile_move_to_folder;
      assert.equal(typeof moveLabel, 'string');
      const moveMenu = await $(
        `//*[@role="menuitem" and contains(normalize-space(.),${xpathLiteral(moveLabel as string)})]`,
      );
      await moveMenu.waitForDisplayed({ timeout: 15_000 });
      await moveMenu.click();

      const targetFolder = await $(
        `//*[@role="menuitem" and contains(normalize-space(.),${xpathLiteral(firstName)})]`,
      );
      await targetFolder.waitForClickable({ timeout: 15_000 });
      await targetFolder.click();

      await browser.waitUntil(
        async () =>
          Boolean(
            (await layoutForTest())?.folders
              .find((f) => f.name === firstName)
              ?.members.includes(groupName),
          ),
        { timeout: 15_000, timeoutMsg: 'Context-menu move did not persist.' },
      );
      const insideFirst = `[data-slot="proxies-folder"][aria-label="${firstName}"] a[data-group-name="${groupName}"]`;
      await $(insideFirst).waitForDisplayed({ timeout: 15_000 });

      const secondFolder = await addFolderFromUi(secondName);
      const firstGroup = await $(insideFirst);
      const secondHeading = await secondFolder.$('button[aria-expanded]');
      await secondHeading.waitForDisplayed({ timeout: 15_000 });

      // Real W3C mouse actions exercise the distance-activated DnD sensor.
      // Do not synthesize DOM drag events or invoke a mutation as acceptance.
      await browser
        .action('pointer', { parameters: { pointerType: 'mouse' } })
        .move({ origin: firstGroup })
        .down()
        .pause(90)
        // The upstream pointer sensor requires 8px of movement. Activate it
        // while still over the source before approaching the destination.
        .move({ origin: firstGroup, x: 22, y: 10, duration: 180 })
        .pause(90)
        .move({ origin: secondHeading, duration: 350 })
        .pause(100)
        .move({ origin: secondHeading, x: 0, y: 3, duration: 90 })
        .pause(100)
        .up()
        .perform();

      await browser.waitUntil(
        async () =>
          Boolean(
            (await layoutForTest())?.folders
              .find((f) => f.name === secondName)
              ?.members.includes(groupName),
          ),
        {
          timeout: 20_000,
          timeoutMsg:
            'Dragging the group onto the second heading did not commit.',
        },
      );

      await browser.refresh();
      await openMainRoute('/main/proxies');
      const movedLink = await $(
        `[data-slot="proxies-folder"][aria-label="${secondName}"] a[data-group-name="${groupName}"]`,
      );
      await movedLink.waitForDisplayed({ timeout: 20_000 });
      const stored = await layoutForTest();
      assert.equal(
        stored?.folders.find((f) => f.name === firstName)?.members.length,
        0,
      );
      assert.deepEqual(
        stored?.folders.find((f) => f.name === secondName)?.members,
        [groupName],
      );
    } catch (error) {
      // The global afterTest snapshot runs after this fixture's restoration.
      // Capture the actual drag failure *before* cleanup changes the page.
      const dir = process.env.CHIMERA_E2E_ARTIFACT_DIR;
      if (dir) {
        const base = path.join(
          dir,
          `proxy-group-drag-before-cleanup-${process.pid}-${Date.now()}`,
        );
        try {
          fs.mkdirSync(dir, { recursive: true });
          await browser.saveScreenshot(`${base}.png`);
          fs.writeFileSync(`${base}.html`, await browser.getPageSource());
        } catch (captureError) {
          console.warn(
            'Could not capture drag failure before cleanup:',
            captureError,
          );
        }
      }
      throw error;
    } finally {
      // The active fixture is isolated, but always restore its layout even if
      // the drag failed or the UI disappeared after a mutation.
      const after = await layoutForTest();
      if (
        after &&
        after.folders.some((folder) =>
          [firstName, secondName].includes(folder.name),
        )
      ) {
        committedValue(
          await invoke('set_proxy_group_layout', {
            profile: uid,
            layout: original ?? { folders: [], ungrouped: [] },
          }),
          'Restore proxy group folder layout after UI acceptance',
        );
      }
      await openMainRoute('/main/proxies');
    }
  });

  it('redirects an existing direct group bookmark to the ref route and keeps its search', async () => {
    const canonicalPath = `/main/proxies/group/${encodeURIComponent(groupName)}`;
    const legacyPath = `/main/proxies/${encodeURIComponent(groupName)}?q=SOCKS`;

    // Direct URL entry is the user-facing action for backwards-compatible
    // bookmarks. The redirect must preserve both group identity and filtering.
    // Open the actual bookmark URL, not openMainRoute(): that helper must
    // observe the *input* path and would intentionally reject a redirect.
    await browser.url(new URL(legacyPath, await browser.getUrl()).href);
    await browser.waitUntil(
      async () => {
        const location = new URL(await browser.getUrl());
        return (
          location.pathname === canonicalPath &&
          location.searchParams.get('q') === 'SOCKS'
        );
      },
      {
        timeout: 15_000,
        timeoutMsg:
          'The previous group URL did not redirect to ref with its search.',
      },
    );
    const selectedNode = await $(
      `[data-slot="proxies-virtual-item"]:has([data-proxy-name="${nodeName}"])`,
    );
    await selectedNode.waitForDisplayed({ timeout: 15_000 });
    assert.equal(
      (await $$('[data-slot="proxies-virtual-item"]')).length,
      1,
      'The preserved search should filter the actual visible node list.',
    );
  });
});
