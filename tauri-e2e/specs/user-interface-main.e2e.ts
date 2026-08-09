import assert from 'node:assert/strict';
import fs from 'node:fs';
import path from 'node:path';

const targetPath = '/main/settings/user-interface';

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

describe('main user interface settings reference layout', () => {
  it('matches the ref group hierarchy without an extra wrapper', async () => {
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
      {
        timeout: 15_000,
        timeoutMsg: 'User interface settings route did not open.',
      },
    );

    const language = await $('[data-slot="language-settings-container"]');
    await language.waitForDisplayed({ timeout: 15_000 });

    const state = await browser.execute(() => {
      const slots = [
        'language-settings-container',
        'theme-mode-settings-container',
        'custom-css-settings-container',
      ];
      const containers = slots.map((slot) =>
        document.querySelector<HTMLElement>(`[data-slot="${slot}"]`),
      );
      const parent = containers[0]?.parentElement ?? null;

      return {
        allContainersPresent: containers.every(Boolean),
        containerTags: containers.map(
          (container) => container?.tagName ?? null,
        ),
        sameParent: containers.every(
          (container) => container?.parentElement === parent,
        ),
        directChildCount: parent?.children.length ?? 0,
        nestedSpaceWrapper: Boolean(
          parent?.querySelector(':scope > .space-y-4 > [data-slot]'),
        ),
        firstMarginBlockEnd: containers[0]
          ? getComputedStyle(containers[0]).marginBlockEnd
          : null,
        secondMarginBlockEnd: containers[1]
          ? getComputedStyle(containers[1]).marginBlockEnd
          : null,
        thirdMarginBlockEnd: containers[2]
          ? getComputedStyle(containers[2]).marginBlockEnd
          : null,
        viewport: { width: innerWidth, height: innerHeight },
      };
    });

    assert.equal(
      state.allContainersPresent,
      true,
      JSON.stringify(state, null, 2),
    );
    assert.deepEqual(state.containerTags, ['DIV', 'DIV', 'DIV']);
    assert.equal(state.sameParent, true, JSON.stringify(state, null, 2));
    assert.equal(state.directChildCount, 3, JSON.stringify(state, null, 2));
    assert.equal(
      state.nestedSpaceWrapper,
      false,
      JSON.stringify(state, null, 2),
    );
    assert.equal(
      state.firstMarginBlockEnd,
      '16px',
      JSON.stringify(state, null, 2),
    );
    assert.equal(
      state.secondMarginBlockEnd,
      '16px',
      JSON.stringify(state, null, 2),
    );
    assert.equal(
      state.thirdMarginBlockEnd,
      '0px',
      JSON.stringify(state, null, 2),
    );

    const evidencePath = process.env.CHIMERA_E2E_EVIDENCE_PATH;
    if (evidencePath) {
      fs.mkdirSync(path.dirname(evidencePath), { recursive: true });
      await browser.saveScreenshot(evidencePath);
    }
  });
});
