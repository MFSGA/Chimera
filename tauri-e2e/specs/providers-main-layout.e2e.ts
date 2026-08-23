import assert from 'node:assert/strict';
import fs from 'node:fs';
import path from 'node:path';

const targetPath = '/main/providers';

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

describe('main providers reference layout', () => {
  before(async () => {
    await browser.setWindowSize(1240, 638);
    await browser.execute(() => {
      localStorage.setItem(btoa('paraglide-language-cache'), 'zh-cn');
    });
    await openMainWindow();
    await browser.setWindowSize(1240, 638);

    await browser.execute((target) => {
      history.pushState({}, '', target);
      window.dispatchEvent(new PopStateEvent('popstate'));
    }, targetPath);
    await browser.waitUntil(
      async () =>
        browser.execute(
          (expected) => location.pathname === expected,
          targetPath,
        ),
      { timeout: 15_000, timeoutMsg: 'Providers route did not open.' },
    );
  });

  it('keeps the ref group rhythm and balanced empty states', async () => {
    const firstGroup = await $('[data-slot="providers-group"]');
    await firstGroup.waitForDisplayed({ timeout: 15_000 });

    const state = await browser.execute(() => {
      const content = document.querySelector<HTMLElement>(
        '[data-slot="providers-content"]',
      );
      const groups = Array.from(
        document.querySelectorAll<HTMLElement>('[data-slot="providers-group"]'),
      );
      const titles = Array.from(
        document.querySelectorAll<HTMLElement>(
          '[data-slot="providers-group-title"]',
        ),
      );
      const emptyCards = groups.map((group) =>
        group.querySelector<HTMLElement>('[data-slot="card"]'),
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
        emptyCards: emptyCards.map(rect),
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
      state.emptyCards.every(
        (card) => !card || (card.height >= 150 && card.width > 700),
      ),
      true,
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
});
