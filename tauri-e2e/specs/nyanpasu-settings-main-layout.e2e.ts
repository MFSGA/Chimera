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

  it('updates ScrollArea direction and sticky title after WebDriver wheel actions', async () => {
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

    // Use a visible point inside the right-hand viewport instead of relying
    // on element-origin conversion. This is the embedded WebDriver wheel path,
    // not trusted OS-wheel input; the DOM only measures target and results.
    const bounds = await browser.execute(() => {
      const element = document.querySelector<HTMLElement>(
        '[data-slot="settings-content-scroll-area"] [data-slot="scroll-area-viewport"]',
      );
      if (!element) throw new Error('Settings viewport disappeared.');
      const rect = element.getBoundingClientRect();
      return { x: rect.x, y: rect.y, width: rect.width, height: rect.height };
    });
    const wheelPoint = {
      x: Math.round(bounds.x + bounds.width * 0.65),
      y: Math.round(bounds.y + bounds.height * 0.6),
    };
    const wheelTarget = await browser.execute(
      (x, y) => {
        const element = document.elementFromPoint(x, y);
        const scrollArea = document.querySelector<HTMLElement>(
          '[data-slot="settings-content-scroll-area"]',
        );
        return {
          insideViewport:
            element?.closest('[data-slot="scroll-area-viewport"]') ===
            scrollArea?.querySelector('[data-slot="scroll-area-viewport"]'),
          tagName: element?.tagName ?? null,
        };
      },
      wheelPoint.x,
      wheelPoint.y,
    );
    assert.equal(
      wheelTarget.insideViewport,
      true,
      'Wheel origin is not inside the Settings scroll viewport: ' +
        JSON.stringify({ bounds, wheelPoint, wheelTarget }),
    );

    // The vendored embedded driver synthesizes an untrusted WheelEvent. First
    // prove that canceling it suppresses the simulated scroll default action.
    await browser.execute(() => {
      const viewport = document.querySelector<HTMLElement>(
        '[data-slot="settings-content-scroll-area"] [data-slot="scroll-area-viewport"]',
      );
      if (!viewport) {
        throw new Error(
          'Settings viewport vanished before wheel cancellation.',
        );
      }
      viewport.dataset.e2eCancelWheelReceived = 'false';
      viewport.dataset.e2eCancelWheelPrevented = 'false';
      viewport.addEventListener(
        'wheel',
        (event) => {
          viewport.dataset.e2eCancelWheelReceived = 'true';
          event.preventDefault();
          viewport.dataset.e2eCancelWheelPrevented = String(
            event.defaultPrevented,
          );
        },
        { once: true, capture: true },
      );
    });
    await browser
      .action('wheel')
      .scroll({
        x: wheelPoint.x,
        y: wheelPoint.y,
        deltaX: 0,
        deltaY: 360,
        duration: 0,
      })
      .perform();
    const afterCancelledWheel = await browser.execute(() => {
      const viewport = document.querySelector<HTMLElement>(
        '[data-slot="settings-content-scroll-area"] [data-slot="scroll-area-viewport"]',
      );
      return {
        scrollTop: viewport?.scrollTop ?? null,
        received: viewport?.dataset.e2eCancelWheelReceived ?? null,
        prevented: viewport?.dataset.e2eCancelWheelPrevented ?? null,
      };
    });
    assert.deepEqual(
      afterCancelledWheel,
      { scrollTop: 0, received: 'true', prevented: 'true' },
      'Canceling the delivered wheel event must suppress nested scrolling.',
    );

    // Diagnostic-only listeners: capture uncanceled WebDriver wheel delivery
    // and transient scrolls. They never synthesize scrolling or set scrollTop.
    await browser.execute(() => {
      const root = document.querySelector<HTMLElement>(
        '[data-slot="settings-content-scroll-area"]',
      );
      const viewport = root?.querySelector<HTMLElement>(
        '[data-slot="scroll-area-viewport"]',
      );
      if (!root || !viewport) {
        throw new Error('Missing scroll diagnostics target');
      }
      root.dataset.e2eWheelReceived = 'false';
      root.dataset.e2eScrollCount = '0';
      root.dataset.e2eMaxScrollTop = '0';
      viewport.addEventListener('scroll', () => {
        root.dataset.e2eScrollCount = String(
          Number(root.dataset.e2eScrollCount ?? 0) + 1,
        );
        root.dataset.e2eMaxScrollTop = String(
          Math.max(
            Number(root.dataset.e2eMaxScrollTop ?? 0),
            viewport.scrollTop,
          ),
        );
      });
      document.addEventListener(
        'wheel',
        (event) => {
          root.dataset.e2eWheelReceived = 'true';
          root.dataset.e2eWheelDelta = String(event.deltaY);
          root.dataset.e2eWheelTrusted = String(event.isTrusted);
          root.dataset.e2eWheelCancelable = String(event.cancelable);
          root.dataset.e2eWheelTarget =
            (event.target as HTMLElement | null)?.tagName ?? 'unknown';
          // Read the final defaultPrevented state after bubbling has finished.
          queueMicrotask(() => {
            root.dataset.e2eWheelCanceled = String(event.defaultPrevented);
          });
        },
        { once: true, capture: true },
      );
    });

    await browser
      .action('pointer')
      .move({ x: wheelPoint.x, y: wheelPoint.y, duration: 0 })
      .perform();
    await browser
      .action('wheel')
      .scroll({
        x: wheelPoint.x,
        y: wheelPoint.y,
        deltaX: 0,
        deltaY: 360,
        duration: 0,
      })
      .perform();

    try {
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
            'The embedded WebDriver wheel-down action did not reveal the compact sticky Settings title.',
        },
      );
    } catch (cause) {
      const observed = await browser.execute(() => {
        const root = document.querySelector<HTMLElement>(
          '[data-slot="settings-content-scroll-area"]',
        );
        const viewport = root?.querySelector<HTMLElement>(
          '[data-slot="scroll-area-viewport"]',
        );
        const titles = Array.from(
          document.querySelectorAll<HTMLElement>(
            '[data-slot="settings-title"]',
          ),
        );
        return {
          scrollTop: viewport?.scrollTop ?? null,
          scrollHeight: viewport?.scrollHeight ?? null,
          clientHeight: viewport?.clientHeight ?? null,
          top: root?.dataset.top ?? null,
          direction: root?.dataset.scrollDirection ?? null,
          wheelReceived: root?.dataset.e2eWheelReceived ?? null,
          wheelDelta: root?.dataset.e2eWheelDelta ?? null,
          wheelTrusted: root?.dataset.e2eWheelTrusted ?? null,
          wheelCancelable: root?.dataset.e2eWheelCancelable ?? null,
          wheelCanceled: root?.dataset.e2eWheelCanceled ?? null,
          wheelTarget: root?.dataset.e2eWheelTarget ?? null,
          scrollEventCount: root?.dataset.e2eScrollCount ?? null,
          maxScrollTop: root?.dataset.e2eMaxScrollTop ?? null,
          viewportOverflowY: viewport
            ? getComputedStyle(viewport).overflowY
            : null,
          titles: titles.map((element) => element.dataset.showTitle),
        };
      });
      throw new Error(
        'Embedded WebDriver wheel-down failed: ' +
          JSON.stringify({ wheelPoint, wheelTarget, observed }),
        { cause },
      );
    }

    await browser
      .action('wheel')
      .scroll({
        x: wheelPoint.x,
        y: wheelPoint.y,
        deltaX: 0,
        deltaY: -1400,
        duration: 0,
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
          'The embedded WebDriver wheel-up action did not restore the expanded Settings title.',
      },
    );
  });
});
