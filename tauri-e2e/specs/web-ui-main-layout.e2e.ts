import assert from 'node:assert/strict';
import fs from 'node:fs';
import path from 'node:path';

const targetPath = '/main/settings/web-ui';

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

describe('main web ui settings reference layout', () => {
  it('keeps the ref container hierarchy and settings rhythm', async () => {
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
        browser.execute((target) => location.pathname === target, targetPath),
      { timeout: 15_000, timeoutMsg: 'Web UI settings route did not open.' },
    );

    await browser.waitUntil(
      async () =>
        browser.execute(() => {
          const containers = Array.from(
            document.querySelectorAll<HTMLElement>(
              '[data-slot="theme-mode-settings-container"]',
            ),
          );
          const parent = containers[0]?.parentElement ?? null;
          return (
            containers.length === 2 &&
            containers.every((container) => container.parentElement === parent)
          );
        }),
      {
        timeout: 15_000,
        timeoutMsg: 'Web UI settings transition did not settle.',
      },
    );

    const state = await browser.execute(() => {
      const containers = Array.from(
        document.querySelectorAll<HTMLElement>(
          '[data-slot="theme-mode-settings-container"]',
        ),
      );
      const parent = containers[0]?.parentElement ?? null;
      const labels = containers.map((container) =>
        container.querySelector<HTMLElement>(
          ':scope > [data-slot="settings-label"]',
        ),
      );
      const groups = containers.map((container) =>
        container.querySelector<HTMLElement>(
          ':scope > [data-slot="settings-group"]',
        ),
      );
      const title = document.querySelector<HTMLElement>(
        '[data-slot="settings-title"]',
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
          lineHeight: computed.lineHeight,
          position: computed.position,
          top: computed.top,
          zIndex: computed.zIndex,
        };
      };

      return {
        viewport: { width: innerWidth, height: innerHeight },
        tags: containers.map((container) => container.tagName),
        sameParent: containers.every(
          (container) => container.parentElement === parent,
        ),
        directChildCount: parent?.children.length ?? 0,
        parentRect: rect(parent),
        parentStyle: style(parent),
        containerRects: containers.map(rect),
        labelStyles: labels.map(style),
        groupStyles: groups.map(style),
        groupDirectChildCounts: groups.map(
          (group) => group?.children.length ?? 0,
        ),
        titleRect: rect(title),
        titleStyle: style(title),
      };
    });

    assert.deepEqual(state.tags, ['DIV', 'DIV']);
    assert.equal(state.sameParent, true, JSON.stringify(state, null, 2));
    assert.equal(state.directChildCount, 2, JSON.stringify(state, null, 2));
    assert.equal(state.parentStyle?.paddingRight, '16px');
    assert.equal(state.parentStyle?.paddingBottom, '16px');
    assert.equal(state.parentStyle?.paddingLeft, '16px');
    assert.equal(state.containerRects.length, 2);
    assert.equal(state.labelStyles.length, 2);
    assert.equal(
      state.labelStyles.every(
        (style) => style?.fontSize === '14px' && style.lineHeight === '20px',
      ),
      true,
      JSON.stringify(state, null, 2),
    );
    assert.equal(state.groupDirectChildCounts[0], 3);
    assert.equal(
      state.groupStyles[0]?.display,
      'flex',
      JSON.stringify(state, null, 2),
    );
    assert.equal(
      state.groupStyles[0]?.gap,
      '4px',
      JSON.stringify(state, null, 2),
    );
    assert.equal(state.titleRect?.height, 64, JSON.stringify(state, null, 2));
    assert.equal(
      state.titleStyle?.position,
      'sticky',
      JSON.stringify(state, null, 2),
    );
    assert.equal(state.titleStyle?.top, '0px', JSON.stringify(state, null, 2));
    assert.equal(
      state.titleStyle?.zIndex,
      '10',
      JSON.stringify(state, null, 2),
    );

    const first = state.containerRects[0];
    const second = state.containerRects[1];
    assert.ok(first && second, JSON.stringify(state, null, 2));
    assert.equal(
      second.y - (first.y + first.height),
      16,
      JSON.stringify(state, null, 2),
    );

    const evidencePath = process.env.CHIMERA_E2E_EVIDENCE_PATH;
    if (evidencePath) {
      fs.mkdirSync(path.dirname(evidencePath), { recursive: true });
      await browser.saveScreenshot(evidencePath);
    }
  });
});
