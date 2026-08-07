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

    const link = await $(`a[href="${targetPath}"]`);
    await link.waitForClickable({ timeout: 15_000 });
    await link.click();
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

      return {
        content: rect(content),
        groups: groups.map(rect),
        titles: titles.map(rect),
        emptyCards: emptyCards.map(rect),
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

    const evidencePath = process.env.CHIMERA_E2E_EVIDENCE_PATH;
    if (evidencePath) {
      fs.mkdirSync(path.dirname(evidencePath), { recursive: true });
      await browser.saveScreenshot(evidencePath);
    }
  });
});
