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

describe('main debug settings reference layout', () => {
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

    const debugLink = await $('a[href="/main/settings/debug"]');
    await debugLink.waitForClickable({ timeout: 15_000 });
    await debugLink.click();
  });

  it('uses the ref debug groups and reveals window debug tools', async () => {
    const advanced = await $('[data-slot="advanced-tools-switch-container"]');
    await advanced.waitForDisplayed({ timeout: 15_000 });

    const switchControl = await advanced.$('button[role="switch"]');
    await switchControl.click();

    const state = await browser.execute(() => {
      const style = (selector: string) => {
        const element = document.querySelector<HTMLElement>(selector);
        if (!element) return null;
        const computed = getComputedStyle(element);
        const rect = element.getBoundingClientRect();
        return {
          width: rect.width,
          height: rect.height,
          display: computed.display,
          gap: computed.gap,
          paddingTop: computed.paddingTop,
          paddingRight: computed.paddingRight,
          paddingBottom: computed.paddingBottom,
          paddingLeft: computed.paddingLeft,
          borderRadius: computed.borderRadius,
          fontSize: computed.fontSize,
          lineHeight: computed.lineHeight,
          fontWeight: computed.fontWeight,
          position: computed.position,
          top: computed.top,
          zIndex: computed.zIndex,
        };
      };

      const containers = document.querySelectorAll<HTMLElement>(
        '[data-slot="debug-settings-container"]',
      );
      const pathGrid = containers[0]?.querySelector<HTMLElement>('.grid');
      const pathButton = pathGrid?.querySelector<HTMLElement>('button');
      const content = containers[0]?.parentElement;
      const settingsTitles = document.querySelectorAll<HTMLElement>(
        '[data-slot="settings-title"]',
      );
      const mainTitle = settingsTitles[1]?.querySelector<HTMLElement>('p');

      return {
        path: location.pathname,
        viewport: { width: innerWidth, height: innerHeight },
        groupCount: containers.length,
        content: content
          ? {
              display: getComputedStyle(content).display,
              rowGap: getComputedStyle(content).rowGap,
              paddingRight: getComputedStyle(content).paddingRight,
              paddingBottom: getComputedStyle(content).paddingBottom,
              paddingLeft: getComputedStyle(content).paddingLeft,
            }
          : null,
        stickyTitle: style('[data-slot="settings-title"]'),
        mainTitle: mainTitle
          ? {
              fontSize: getComputedStyle(mainTitle).fontSize,
              lineHeight: getComputedStyle(mainTitle).lineHeight,
              fontWeight: getComputedStyle(mainTitle).fontWeight,
            }
          : null,
        pathGrid: pathGrid
          ? {
              display: getComputedStyle(pathGrid).display,
              columns: getComputedStyle(pathGrid).gridTemplateColumns,
              gap: getComputedStyle(pathGrid).gap,
            }
          : null,
        pathButton: pathButton
          ? {
              height: pathButton.getBoundingClientRect().height,
              borderRadius: getComputedStyle(pathButton).borderRadius,
              paddingLeft: getComputedStyle(pathButton).paddingLeft,
              paddingRight: getComputedStyle(pathButton).paddingRight,
              fontWeight: getComputedStyle(pathButton).fontWeight,
            }
          : null,
        label: style('[data-slot="settings-label"]'),
        group: style('[data-slot="settings-group"]'),
        cardContent: style('[data-slot="settings-card-content"]'),
        hasWindowDebug: document.body.innerText.includes('Window Debug Utils'),
        hasWindowLabel: document.body.innerText.includes(
          'Current Window Label:',
        ),
        hasEditorButton: document.body.innerText.includes(
          'Create Test Editor Window',
        ),
        hasTrayButton: document.body.innerText.includes(
          'Create Persistent Tray Menu Window',
        ),
      };
    });

    const evidencePath = process.env.CHIMERA_E2E_EVIDENCE_PATH;
    if (evidencePath) {
      fs.mkdirSync(path.dirname(evidencePath), { recursive: true });
      await browser.saveScreenshot(evidencePath);
    }

    assert.equal(state.path, '/main/settings/debug');
    assert.equal(state.groupCount, 2, JSON.stringify(state, null, 2));
    assert.deepEqual(
      state.viewport,
      { width: 1224, height: 629 },
      JSON.stringify(state, null, 2),
    );
    assert.deepEqual(
      state.content,
      {
        display: 'block',
        rowGap: 'normal',
        paddingRight: '16px',
        paddingBottom: '16px',
        paddingLeft: '16px',
      },
      JSON.stringify(state, null, 2),
    );
    assert.equal(state.stickyTitle?.height, 64, JSON.stringify(state, null, 2));
    assert.equal(
      state.stickyTitle?.position,
      'sticky',
      JSON.stringify(state, null, 2),
    );
    assert.equal(state.stickyTitle?.top, '0px', JSON.stringify(state, null, 2));
    assert.equal(
      state.stickyTitle?.zIndex,
      '10',
      JSON.stringify(state, null, 2),
    );
    assert.deepEqual(
      state.mainTitle,
      { fontSize: '30px', lineHeight: '36px', fontWeight: '700' },
      JSON.stringify(state, null, 2),
    );
    assert.equal(
      state.pathGrid?.display,
      'grid',
      JSON.stringify(state, null, 2),
    );
    assert.equal(state.pathGrid?.gap, '8px', JSON.stringify(state, null, 2));
    assert.equal(
      state.pathGrid?.columns.split(' ').length,
      4,
      JSON.stringify(state, null, 2),
    );
    assert.deepEqual(
      state.pathButton,
      {
        height: 72,
        borderRadius: '24px',
        paddingLeft: '20px',
        paddingRight: '20px',
        fontWeight: '700',
      },
      JSON.stringify(state, null, 2),
    );
    assert.deepEqual(
      {
        fontSize: state.label?.fontSize,
        paddingTop: state.label?.paddingTop,
        paddingRight: state.label?.paddingRight,
        paddingBottom: state.label?.paddingBottom,
        paddingLeft: state.label?.paddingLeft,
      },
      {
        fontSize: '14px',
        paddingTop: '12px',
        paddingRight: '12px',
        paddingBottom: '12px',
        paddingLeft: '12px',
      },
      JSON.stringify(state, null, 2),
    );
    assert.equal(state.group?.gap, '4px', JSON.stringify(state, null, 2));
    assert.deepEqual(
      {
        paddingTop: state.cardContent?.paddingTop,
        paddingRight: state.cardContent?.paddingRight,
        paddingBottom: state.cardContent?.paddingBottom,
        paddingLeft: state.cardContent?.paddingLeft,
      },
      {
        paddingTop: '24px',
        paddingRight: '20px',
        paddingBottom: '24px',
        paddingLeft: '20px',
      },
      JSON.stringify(state, null, 2),
    );
    assert.equal(state.hasWindowDebug, true, JSON.stringify(state, null, 2));
    assert.equal(state.hasWindowLabel, true, JSON.stringify(state, null, 2));
    assert.equal(state.hasEditorButton, true, JSON.stringify(state, null, 2));
    assert.equal(state.hasTrayButton, true, JSON.stringify(state, null, 2));
  });
});
