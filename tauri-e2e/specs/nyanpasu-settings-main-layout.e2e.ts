import assert from 'node:assert/strict';
import fs from 'node:fs';
import path from 'node:path';
import { openMainRoute } from './main-window.js';

describe('main Chimera settings reference layout', () => {
  before(async () => {
    await browser.setWindowSize(1240, 638);
    await browser.execute(() => {
      localStorage.setItem(btoa('paraglide-language-cache'), 'zh-cn');
    });
    await openMainRoute('/main/settings/chimera');
    await browser.setWindowSize(1240, 638);
    await browser.waitUntil(
      async () =>
        browser.execute(() => {
          const groups = Array.from(
            document.querySelectorAll<HTMLElement>(
              '[data-slot="app-settings-container"]',
            ),
          );
          return (
            groups.length === 6 &&
            groups.every(
              (group) =>
                group.querySelector('[data-slot="settings-label"]') &&
                group.querySelector('[data-slot="settings-group"]'),
            ) &&
            document.querySelector('[data-slot="settings-title"]') !== null
          );
        }),
      {
        timeout: 30_000,
        timeoutMsg: 'Chimera settings reference DOM did not render.',
      },
    );
  });

  it('matches the ref settings-group DOM and spacing contract', async () => {
    const firstGroup = await $('[data-slot="app-settings-container"]');
    await firstGroup.waitForDisplayed({ timeout: 15_000 });

    const state = await browser.execute(() => {
      const groups = Array.from(
        document.querySelectorAll<HTMLElement>(
          '[data-slot="app-settings-container"]',
        ),
      );
      const title = document.querySelector<HTMLElement>(
        '[data-slot="settings-title"]',
      );
      const content = groups[0]?.parentElement;
      const labels = groups.map((group) =>
        group.querySelector<HTMLElement>('[data-slot="settings-label"]'),
      );
      const settingsGroups = groups.map((group) =>
        group.querySelector<HTMLElement>('[data-slot="settings-group"]'),
      );

      if (
        !title ||
        !content ||
        groups.length !== 6 ||
        labels.some((label) => !label) ||
        settingsGroups.some((group) => !group)
      ) {
        throw new Error('Chimera settings reference DOM is incomplete.');
      }

      const titleStyle = getComputedStyle(title);
      const contentStyle = getComputedStyle(content);
      const firstLabelStyle = getComputedStyle(labels[0]!);
      const firstSettingsGroupStyle = getComputedStyle(settingsGroups[0]!);
      const firstGroupRect = groups[0]!.getBoundingClientRect();
      const secondGroupRect = groups[1]!.getBoundingClientRect();

      return {
        path: location.pathname,
        viewport: { width: innerWidth, height: innerHeight },
        groupCount: groups.length,
        groupTags: groups.map((group) => group.tagName),
        directSlots: groups.map((group) =>
          Array.from(group.children).map((child) =>
            child.getAttribute('data-slot'),
          ),
        ),
        title: {
          height: titleStyle.height,
          position: titleStyle.position,
          top: titleStyle.top,
          zIndex: titleStyle.zIndex,
        },
        content: {
          paddingLeft: contentStyle.paddingLeft,
          paddingRight: contentStyle.paddingRight,
          paddingBottom: contentStyle.paddingBottom,
        },
        groupSpacing: {
          gap: secondGroupRect.top - firstGroupRect.bottom,
        },
        label: {
          fontSize: firstLabelStyle.fontSize,
          lineHeight: firstLabelStyle.lineHeight,
        },
        settingsGroup: {
          display: firstSettingsGroupStyle.display,
          rowGap: firstSettingsGroupStyle.rowGap,
        },
      };
    });

    assert.equal(state.path, '/main/settings/chimera');
    assert.ok(state.viewport.width >= 1200, JSON.stringify(state, null, 2));
    assert.ok(state.viewport.height >= 600, JSON.stringify(state, null, 2));
    assert.equal(state.groupCount, 6);
    assert.deepEqual(
      state.groupTags,
      Array.from({ length: 6 }, () => 'DIV'),
    );
    assert.deepEqual(
      state.directSlots,
      Array.from({ length: 6 }, () => ['settings-label', 'settings-group']),
    );
    assert.deepEqual(state.title, {
      height: '64px',
      position: 'sticky',
      top: '0px',
      zIndex: '10',
    });
    assert.deepEqual(state.content, {
      paddingLeft: '16px',
      paddingRight: '16px',
      paddingBottom: '16px',
    });
    assert.deepEqual(state.groupSpacing, {
      gap: 16,
    });
    assert.match(state.label.fontSize, /^\d+(?:\.\d+)?px$/);
    assert.match(state.label.lineHeight, /^\d+(?:\.\d+)?px$/);
    assert.equal(state.settingsGroup.display, 'flex');
    assert.equal(state.settingsGroup.rowGap, '4px');

    const evidencePath = process.env.CHIMERA_E2E_EVIDENCE_PATH;
    if (evidencePath) {
      fs.mkdirSync(path.dirname(evidencePath), { recursive: true });
      await browser.saveScreenshot(evidencePath);
    }
  });

  it('updates ScrollArea direction and sticky title on actual wheel input', async () => {
    const viewport = await $(
      '[data-slot="settings-content-scroll-area"] [data-slot="scroll-area-viewport"]',
    );
    await viewport.waitForDisplayed({ timeout: 15_000 });

    const initial = await browser.execute(() => {
      const root = document.querySelector<HTMLElement>(
        '[data-slot="settings-content-scroll-area"]',
      );
      const element = root?.querySelector<HTMLElement>(
        '[data-slot="scroll-area-viewport"]',
      );
      if (!root || !element) {
        throw new Error('The real settings ScrollArea viewport is missing.');
      }
      return {
        availableScroll: element.scrollHeight - element.clientHeight,
        scrollTop: element.scrollTop,
        top: root.dataset.top,
      };
    });
    assert.ok(
      initial.availableScroll > 120,
      'Settings page must actually overflow before wheel testing: ' +
        JSON.stringify(initial),
    );
    assert.equal(initial.scrollTop, 0, JSON.stringify(initial));
    assert.equal(initial.top, 'true');

    // The failed runner snapshot was still at scrollTop=0. Target a point
    // within visible scroll content instead of relying on the element center,
    // which may overlap an interactive control. Use real W3C wheel input;
    // do not set scrollTop or dispatch synthetic events.
    const wheelOrigin = await browser.execute(() => {
      const element = document.querySelector<HTMLElement>(
        '[data-slot="settings-content-scroll-area"] [data-slot="scroll-area-viewport"]',
      );
      if (!element) throw new Error('Settings viewport disappeared.');
      const rect = element.getBoundingClientRect();
      const x = Math.round(rect.left + rect.width * 0.42);
      const y = Math.round(rect.top + rect.height * 0.83);
      const hit = document.elementFromPoint(x, y);
      return {
        x,
        y,
        inside: hit === element || element.contains(hit),
        hit: hit?.tagName ?? null,
        slot: hit?.getAttribute('data-slot') ?? null,
      };
    });
    assert.equal(
      wheelOrigin.inside,
      true,
      'Wheel origin must hit the visible Settings scroll content: ' +
        JSON.stringify(wheelOrigin),
    );
    console.log('[ScrollArea E2E] wheel origin:', wheelOrigin);
    await browser
      .action('wheel')
      .scroll({
        // With no element origin WDIO uses viewport-relative CSS coordinates.
        x: wheelOrigin.x,
        y: wheelOrigin.y,
        deltaX: 0,
        deltaY: 480,
        duration: 300,
      })
      .perform();

    const afterWheel = await browser.execute(() => {
      const root = document.querySelector<HTMLElement>(
        '[data-slot="settings-content-scroll-area"]',
      );
      const element = root?.querySelector<HTMLElement>(
        '[data-slot="scroll-area-viewport"]',
      );
      const titles = document.querySelectorAll<HTMLElement>(
        '[data-slot="settings-title"]',
      );
      return {
        scrollTop: element?.scrollTop ?? null,
        top: root?.dataset.top ?? null,
        direction: root?.dataset.scrollDirection ?? null,
        compactTitle: titles[0]?.dataset.showTitle ?? null,
        expandedTitle: titles[1]?.dataset.showTitle ?? null,
      };
    });
    console.log('[ScrollArea E2E] after wheel down:', afterWheel);

    await browser.waitUntil(
      async () =>
        browser.execute(() => {
          const root = document.querySelector<HTMLElement>(
            '[data-slot="settings-content-scroll-area"]',
          );
          const element = root?.querySelector<HTMLElement>(
            '[data-slot="scroll-area-viewport"]',
          );
          const titles = document.querySelectorAll<HTMLElement>(
            '[data-slot="settings-title"]',
          );
          return (
            (element?.scrollTop ?? 0) > 40 &&
            root?.dataset.top === 'false' &&
            root?.dataset.scrollDirection === 'down' &&
            titles[0]?.dataset.showTitle === 'true' &&
            titles[1]?.dataset.showTitle === 'false'
          );
        }),
      {
        timeout: 15_000,
        timeoutMsg:
          'A real wheel-down action did not reveal the compact sticky Settings title.',
      },
    );

    await browser
      .action('wheel')
      .scroll({
        x: wheelOrigin.x,
        y: wheelOrigin.y,
        deltaX: 0,
        deltaY: -1800,
        duration: 300,
      })
      .perform();

    await browser.waitUntil(
      async () =>
        browser.execute(() => {
          const root = document.querySelector<HTMLElement>(
            '[data-slot="settings-content-scroll-area"]',
          );
          const element = root?.querySelector<HTMLElement>(
            '[data-slot="scroll-area-viewport"]',
          );
          const titles = document.querySelectorAll<HTMLElement>(
            '[data-slot="settings-title"]',
          );
          return (
            element?.scrollTop === 0 &&
            root?.dataset.top === 'true' &&
            root?.dataset.scrollDirection === 'up' &&
            titles[0]?.dataset.showTitle === 'false' &&
            titles[1]?.dataset.showTitle === 'true'
          );
        }),
      {
        timeout: 15_000,
        timeoutMsg:
          'A real wheel-up action did not restore the expanded Settings title.',
      },
    );
  });
});
