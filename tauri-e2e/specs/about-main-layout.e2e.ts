import assert from 'node:assert/strict';
import fs from 'node:fs';
import path from 'node:path';

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

describe('main about settings reference layout', () => {
  before(async () => {
    await browser.setWindowSize(1240, 638);
    await browser.execute(() => {
      localStorage.setItem(btoa('paraglide-language-cache'), 'zh-cn');
    });
    await openMainWindow();
    await browser.setWindowSize(1240, 638);

    const settingsLink = await $('a[href="/main/settings/system"]');
    await settingsLink.waitForClickable({ timeout: 15_000 });
    await settingsLink.click();

    const aboutLink = await $('a[href="/main/settings/about"]');
    await aboutLink.waitForClickable({ timeout: 15_000 });
    await aboutLink.click();
  });

  it('matches the ref title, grid, and version-card CSS contract', async () => {
    const card = await $('[data-slot="settings-card"]');
    await card.waitForDisplayed({ timeout: 15_000 });

    const state = await browser.execute(() => {
      const titleSlots = Array.from(
        document.querySelectorAll<HTMLElement>('[data-slot="settings-title"]'),
      );
      const stickyTitle = titleSlots[0];
      const mainTitle = titleSlots[1];
      const visibleTitle = mainTitle?.querySelector<HTMLElement>('p');
      const versionCard = Array.from(
        document.querySelectorAll<HTMLElement>('[data-slot="settings-card"]'),
      ).find((candidate) => {
        const parent = candidate.parentElement;
        return parent && getComputedStyle(parent).display === 'grid';
      });
      const grid = versionCard?.parentElement;
      const pageContent = grid?.parentElement;

      if (
        !stickyTitle ||
        !mainTitle ||
        !visibleTitle ||
        !versionCard ||
        !grid ||
        !pageContent
      ) {
        throw new Error('About settings reference DOM is incomplete.');
      }

      const stickyStyle = getComputedStyle(stickyTitle);
      const mainStyle = getComputedStyle(mainTitle);
      const titleStyle = getComputedStyle(visibleTitle);
      const gridStyle = getComputedStyle(grid);
      const contentStyle = getComputedStyle(pageContent);
      const cardStyle = getComputedStyle(versionCard);

      return {
        path: location.pathname,
        viewport: { width: innerWidth, height: innerHeight },
        sticky: {
          height: stickyStyle.height,
          paddingLeft: stickyStyle.paddingLeft,
          paddingRight: stickyStyle.paddingRight,
          position: stickyStyle.position,
          top: stickyStyle.top,
          zIndex: stickyStyle.zIndex,
        },
        mainTitle: {
          height: mainStyle.height,
          paddingLeft: mainStyle.paddingLeft,
          paddingTop: mainStyle.paddingTop,
          paddingBottom: mainStyle.paddingBottom,
        },
        title: {
          fontSize: titleStyle.fontSize,
          fontWeight: titleStyle.fontWeight,
          lineHeight: titleStyle.lineHeight,
        },
        content: {
          paddingLeft: contentStyle.paddingLeft,
          paddingRight: contentStyle.paddingRight,
          paddingBottom: contentStyle.paddingBottom,
        },
        grid: {
          display: gridStyle.display,
          gap: gridStyle.gap,
          columns: gridStyle.gridTemplateColumns,
        },
        card: {
          borderRadius: cardStyle.borderRadius,
          width: cardStyle.width,
        },
      };
    });

    assert.equal(state.path, '/main/settings/about');
    assert.deepEqual(state.viewport, { width: 1224, height: 629 });
    assert.deepEqual(state.sticky, {
      height: '64px',
      paddingLeft: '24px',
      paddingRight: '24px',
      position: 'sticky',
      top: '0px',
      zIndex: '10',
    });
    assert.deepEqual(state.mainTitle, {
      height: '96px',
      paddingLeft: '24px',
      paddingTop: '40px',
      paddingBottom: '16px',
    });
    assert.deepEqual(state.title, {
      fontSize: '30px',
      fontWeight: '700',
      lineHeight: '36px',
    });
    assert.deepEqual(state.content, {
      paddingLeft: '16px',
      paddingRight: '16px',
      paddingBottom: '16px',
    });
    assert.equal(state.grid.display, 'grid');
    assert.equal(state.grid.gap, '8px');
    assert.equal(state.grid.columns.split(' ').length, 2);
    assert.match(state.card.borderRadius, /^\d+(?:\.\d+)?px$/);

    const evidencePath = process.env.CHIMERA_E2E_EVIDENCE_PATH;
    if (evidencePath) {
      fs.mkdirSync(path.dirname(evidencePath), { recursive: true });
      await browser.saveScreenshot(evidencePath);
    }
  });
});
