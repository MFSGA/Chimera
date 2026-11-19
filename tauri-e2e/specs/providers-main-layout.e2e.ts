import assert from 'node:assert/strict';
import fs from 'node:fs';
import path from 'node:path';
import { openMainRoute } from './main-window.js';

const targetPath = '/main/providers';

describe('main providers reference layout', () => {
  before(async () => {
    await browser.setWindowSize(1240, 638);
    await browser.execute(() => {
      localStorage.setItem(btoa('paraglide-language-cache'), 'zh-cn');
    });
    await openMainRoute(targetPath);
    await browser.setWindowSize(1240, 638);
  });

  it('keeps the ref group rhythm and balanced empty states', async () => {
    await browser.waitUntil(
      async () =>
        browser.execute(() => {
          const content = document.querySelector<HTMLElement>(
            '[data-slot="providers-content"]',
          );
          if (!content) return false;

          const rect = content.getBoundingClientRect();
          return (
            rect.right > 0 &&
            rect.left < window.innerWidth &&
            content.querySelectorAll('[data-slot="providers-group"]').length ===
              2
          );
        }),
      {
        timeout: 15_000,
        timeoutMsg: 'The active Providers route did not render.',
      },
    );

    const state = await browser.execute(() => {
      const content = document.querySelector<HTMLElement>(
        '[data-slot="providers-content"]',
      );
      const groups = Array.from(
        content?.querySelectorAll<HTMLElement>(
          '[data-slot="providers-group"]',
        ) ?? [],
      );
      const titles = Array.from(
        content?.querySelectorAll<HTMLElement>(
          '[data-slot="providers-group-title"]',
        ) ?? [],
      );
      const emptyStates = groups.map((group) =>
        group.querySelector<HTMLElement>('[data-slot="empty-state"]'),
      );

      const rect = (element: HTMLElement | null) =>
        element
          ? {
              x: Math.round(element.getBoundingClientRect().x),
              y: Math.round(element.getBoundingClientRect().y),
              width: Math.round(element.getBoundingClientRect().width),
              height: Math.round(element.getBoundingClientRect().height),
            }
          : null;

      const style = (element: HTMLElement | null) => {
        if (!element) return null;
        const computed = getComputedStyle(element);
        return {
          display: computed.display,
          gap: computed.gap,
          paddingTop: computed.paddingTop,
          paddingRight: computed.paddingRight,
          paddingBottom: computed.paddingBottom,
          paddingLeft: computed.paddingLeft,
          fontSize: computed.fontSize,
          fontWeight: computed.fontWeight,
          position: computed.position,
          top: computed.top,
          zIndex: computed.zIndex,
          gridTemplateColumns: computed.gridTemplateColumns,
        };
      };

      const groupContents = groups.map((group) =>
        group.querySelector<HTMLElement>(
          '[data-slot="providers-group-content"]',
        ),
      );

      const page = groups[0]?.parentElement ?? null;

      return {
        content: rect(content),
        contentStyle: style(page),
        groups: groups.map(rect),
        groupStyles: groups.map(style),
        titles: titles.map(rect),
        titleStyles: titles.map(style),
        emptyStates: emptyStates.map((empty) => ({
          rect: rect(empty),
          role: empty?.getAttribute('role') ?? null,
          message:
            empty
              ?.querySelector('[data-slot="empty-state-message"]')
              ?.textContent?.trim() ?? '',
        })),
        groupContentStyles: groupContents.map(style),
        viewport: { width: window.innerWidth, height: window.innerHeight },
      };
    });

    assert.ok(state.content, JSON.stringify(state, null, 2));
    assert.equal(state.groups.length, 2, JSON.stringify(state, null, 2));
    assert.equal(state.titles.length, 2, JSON.stringify(state, null, 2));
    assert.equal(
      state.titles.every((title) => title?.height === 64),
      true,
    );
    assert.equal(
      state.groups.every((group) => (group?.width ?? 0) > 700),
      true,
    );
    assert.equal(
      state.emptyStates.every((empty, index) =>
        state.groupContentStyles[index]
          ? empty.rect == null
          : (empty.rect?.height ?? 0) >= 150 &&
            (empty.rect?.width ?? 0) > 700 &&
            empty.role === 'status' &&
            empty.message.length > 0,
      ),
      true,
      `Providers must show the ref empty state or an actual grid: ${JSON.stringify(state, null, 2)}`,
    );
    assert.equal((state.groups[0]?.x ?? 0) - (state.content?.x ?? 0), 16);
    assert.deepEqual(state.contentStyle, {
      display: 'flex',
      gap: '16px',
      paddingTop: '0px',
      paddingRight: '16px',
      paddingBottom: '16px',
      paddingLeft: '16px',
      fontSize: '16px',
      fontWeight: '400',
      position: 'static',
      top: 'auto',
      zIndex: 'auto',
      gridTemplateColumns: 'none',
    });
    assert.equal(
      state.groupStyles.every(
        (style) => style?.display === 'flex' && style.gap === '4px',
      ),
      true,
      JSON.stringify(state, null, 2),
    );
    assert.equal(
      state.titleStyles.every(
        (style) =>
          style?.display === 'flex' &&
          style.fontSize === '18px' &&
          style.fontWeight === '600' &&
          style.position === 'sticky' &&
          style.top === '0px' &&
          style.zIndex === '10',
      ),
      true,
      JSON.stringify(state, null, 2),
    );
    assert.equal(
      state.groupContentStyles.every(
        (style) =>
          !style ||
          (style.display === 'grid' &&
            style.gap === '8px' &&
            style.gridTemplateColumns !== 'none'),
      ),
      true,
      JSON.stringify(state, null, 2),
    );

    const evidencePath = process.env.CHIMERA_E2E_EVIDENCE_PATH;
    if (evidencePath) {
      fs.mkdirSync(path.dirname(evidencePath), { recursive: true });
      await browser.saveScreenshot(evidencePath);
    }
  });

  it('keeps empty-section refresh controls clickable without changing routes', async () => {
    const providerActions = [
      '[data-slot="providers-update-all-proxies"]',
      '[data-slot="providers-update-all-rules"]',
    ];
    const actions = await Promise.all(
      providerActions.map((selector) => $(selector)),
    );
    for (const action of actions) {
      await action.waitForDisplayed({ timeout: 15_000 });
    }

    // Fresh isolated E2E state has no subscription or rules provider to
    // mutate. Test the actual UI click path without requesting a network update.
    const groups = await $$('[data-slot="providers-group-content"]');
    assert.equal(
      groups.length,
      0,
      'The empty-state fixture unexpectedly contains providers.',
    );
    const before = new URL(await browser.getUrl());
    for (const action of actions) {
      await action.click();
      assert.equal(new URL(await browser.getUrl()).pathname, before.pathname);
    }
    const statuses = await $$('[data-slot="empty-state"][role="status"]');
    assert.equal(statuses.length, 2);
  });
});
