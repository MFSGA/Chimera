import assert from 'node:assert/strict';
import fs from 'node:fs';
import path from 'node:path';

const targetPath = '/main/settings/system';

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

describe('main settings reference layout', () => {
  before(async () => {
    await browser.setWindowSize(1240, 638);
    await browser.execute(() => {
      localStorage.setItem(btoa('paraglide-language-cache'), 'zh-cn');
    });
    await openMainWindow();
    await browser.setWindowSize(1240, 638);

    const link = await $(`a[href="${targetPath}"]`);
    await link.waitForClickable({ timeout: 15_000 });
    await link.click();
    await browser.waitUntil(
      async () =>
        browser.execute(
          (expected) => location.pathname === expected,
          targetPath,
        ),
      { timeout: 15_000, timeoutMsg: 'System settings route did not open.' },
    );
  });

  it('keeps the ref sidebar and full-height flex content chain', async () => {
    const content = await $('[data-slot="settings-content"]');
    await content.waitForDisplayed({ timeout: 15_000 });

    const state = await browser.execute(() => {
      const container = document.querySelector<HTMLElement>(
        '[data-slot="settings-container"]',
      );
      const sidebar = document.querySelector<HTMLElement>(
        '[data-slot="settings-sidebar-scroll-area"]',
      );
      const content = document.querySelector<HTMLElement>(
        '[data-slot="settings-content"]',
      );
      const animatedOutlet = content?.firstElementChild as HTMLElement | null;
      const routeContent =
        animatedOutlet?.firstElementChild as HTMLElement | null;
      const systemLink = sidebar?.querySelector<HTMLElement>(
        'a[href="/main/settings/system"]',
      );
      const systemLabel = systemLink?.querySelector<HTMLElement>(
        '.text-sm.font-medium',
      );
      const systemDescription =
        systemLink?.querySelector<HTMLElement>('[class*="text-xs"]');
      const externalCoreIcon = sidebar?.querySelector<HTMLImageElement>(
        'a[href="/main/settings/web-ui"] img',
      );
      const zincProbe = document.createElement('div');
      zincProbe.className = 'text-zinc-500';
      document.body.append(zincProbe);
      const zinc500Color = getComputedStyle(zincProbe).color;
      zincProbe.remove();
      const rect = (element: HTMLElement | null) =>
        element
          ? {
              x: Math.round(element.getBoundingClientRect().x),
              y: Math.round(element.getBoundingClientRect().y),
              width: Math.round(element.getBoundingClientRect().width),
              height: Math.round(element.getBoundingClientRect().height),
            }
          : null;
      const style = (element: HTMLElement | null) =>
        element
          ? {
              display: getComputedStyle(element).display,
              flexDirection: getComputedStyle(element).flexDirection,
              flexGrow: getComputedStyle(element).flexGrow,
            }
          : null;

      return {
        viewport: { width: innerWidth, height: innerHeight },
        container: rect(container),
        sidebar: rect(sidebar),
        content: rect(content),
        animatedOutlet: rect(animatedOutlet),
        animatedOutletStyle: style(animatedOutlet),
        routeContent: rect(routeContent),
        systemLink: rect(systemLink ?? null),
        externalCoreIcon: rect(externalCoreIcon ?? null),
        systemLabelStyle: systemLabel
          ? {
              textOverflow: getComputedStyle(systemLabel).textOverflow,
              whiteSpace: getComputedStyle(systemLabel).whiteSpace,
            }
          : null,
        systemDescriptionColor: systemDescription
          ? getComputedStyle(systemDescription).color
          : null,
        zinc500Color,
      };
    });

    assert.ok(state.viewport.width >= 1200, JSON.stringify(state, null, 2));
    assert.ok(state.viewport.height >= 600, JSON.stringify(state, null, 2));
    assert.ok(state.container, JSON.stringify(state, null, 2));
    assert.ok(state.sidebar, JSON.stringify(state, null, 2));
    assert.ok(state.content, JSON.stringify(state, null, 2));
    assert.ok(state.animatedOutlet, JSON.stringify(state, null, 2));
    assert.equal(
      state.animatedOutletStyle?.display,
      'flex',
      JSON.stringify(state, null, 2),
    );
    assert.equal(
      state.animatedOutletStyle?.flexDirection,
      'column',
      JSON.stringify(state, null, 2),
    );
    assert.equal(
      state.animatedOutletStyle?.flexGrow,
      '1',
      JSON.stringify(state, null, 2),
    );
    assert.ok(
      (state.animatedOutlet?.height ?? 0) >= (state.content?.height ?? 0) - 2,
      JSON.stringify(state, null, 2),
    );
    assert.ok(state.routeContent, JSON.stringify(state, null, 2));
    assert.ok(state.systemLink, JSON.stringify(state, null, 2));
    assert.equal(
      state.externalCoreIcon?.width,
      30,
      JSON.stringify(state, null, 2),
    );
    assert.equal(
      state.externalCoreIcon?.height,
      30,
      JSON.stringify(state, null, 2),
    );
    assert.notEqual(
      state.systemLabelStyle?.textOverflow,
      'ellipsis',
      JSON.stringify(state, null, 2),
    );
    assert.notEqual(
      state.systemLabelStyle?.whiteSpace,
      'nowrap',
      JSON.stringify(state, null, 2),
    );
    assert.equal(
      state.systemDescriptionColor,
      state.zinc500Color,
      JSON.stringify(state, null, 2),
    );

    const evidencePath = process.env.CHIMERA_E2E_EVIDENCE_PATH;
    if (evidencePath) {
      fs.mkdirSync(path.dirname(evidencePath), { recursive: true });
      await browser.saveScreenshot(evidencePath);
    }
  });
});
