import assert from 'node:assert/strict';
import fs from 'node:fs';
import path from 'node:path';

const systemPath = '/main/settings/system';
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

async function waitForPath(pathname: string) {
  await browser.waitUntil(
    async () =>
      browser.execute((expected) => location.pathname === expected, pathname),
    {
      timeout: 15_000,
      timeoutMsg: `Navigation to ${pathname} did not complete.`,
    },
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

describe('main ref input primitive', () => {
  before(async () => {
    await browser.setWindowSize(1240, 638);
    await browser.execute(() => {
      localStorage.setItem(btoa('paraglide-language-cache'), 'zh-cn');
    });
    await openMainWindow();
    await browser.setWindowSize(1240, 638);

    const settingsLink = await $(`a[href="${systemPath}"]`);
    await settingsLink.waitForClickable({ timeout: 15_000 });
    await settingsLink.click();
    await waitForPath(systemPath);

    const webUiLink = await $(`a[href="${targetPath}"]`);
    await webUiLink.waitForClickable({ timeout: 15_000 });
    await webUiLink.click();
    await waitForPath(targetPath);
  });

  it('uses the ref outlined fieldset, floating label, and hidden filled line', async () => {
    const card = await $('[data-slot="external-controller-config-card"]');
    await card.waitForDisplayed({ timeout: 15_000 });

    const trigger = await card.$('[data-slot="modal-trigger"]');
    await trigger.waitForClickable({ timeout: 15_000 });
    const placeholderContainer = await trigger.$(
      '[data-slot="modal-trigger-placeholder-container"]',
    );
    const placeholder = await trigger.$(
      '[data-slot="modal-trigger-placeholder"]',
    );
    assert.equal(await placeholderContainer.isExisting(), true);
    assert.equal(await placeholder.isExisting(), true);
    await trigger.click();

    const modal = await $('[data-slot="modal-content"]');
    await modal.waitForDisplayed({ timeout: 15_000 });

    const input = await modal.$('input');
    await input.waitForDisplayed({ timeout: 15_000 });
    await input.click();
    await browser.pause(500);

    const state = await browser.execute(() => {
      const modal = document.querySelector<HTMLElement>(
        '[data-slot="modal-content"]',
      );
      const input = modal?.querySelector<HTMLInputElement>('input') ?? null;
      const container = input?.parentElement ?? null;
      const fieldset =
        container?.querySelector<HTMLElement>('fieldset') ?? null;
      const label = container?.querySelector<HTMLElement>('label') ?? null;
      const line = container?.lastElementChild as HTMLElement | null;

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
        viewport: { width: innerWidth, height: innerHeight },
        value: input?.value ?? null,
        container: rect(container),
        input: rect(input),
        fieldset: rect(fieldset),
        label: rect(label),
        containerDisplay: container
          ? getComputedStyle(container).display
          : null,
        containerHeight: container ? getComputedStyle(container).height : null,
        containerTransform: container
          ? getComputedStyle(container).transform
          : null,
        fieldsetDisplay: fieldset ? getComputedStyle(fieldset).display : null,
        fieldsetBorderWidth: fieldset
          ? getComputedStyle(fieldset).borderTopWidth
          : null,
        fieldsetBorderStyle: fieldset
          ? getComputedStyle(fieldset).borderTopStyle
          : null,
        labelFontSize: label ? getComputedStyle(label).fontSize : null,
        lineDisplay: line ? getComputedStyle(line).display : null,
      };
    });

    assert.ok(state.viewport.width >= 1200, JSON.stringify(state, null, 2));
    assert.ok(state.viewport.height >= 600, JSON.stringify(state, null, 2));
    assert.ok(state.value, JSON.stringify(state, null, 2));
    assert.equal(state.containerHeight, '56px', JSON.stringify(state, null, 2));
    assert.equal(state.container?.height, 56, JSON.stringify(state, null, 2));
    assert.ok(
      (state.container?.width ?? 0) >= 300,
      JSON.stringify(state, null, 2),
    );
    assert.equal(
      state.containerDisplay,
      'flex',
      JSON.stringify(state, null, 2),
    );
    assert.ok(state.fieldset, JSON.stringify(state, null, 2));
    assert.notEqual(
      state.fieldsetDisplay,
      'none',
      JSON.stringify(state, null, 2),
    );
    assert.equal(
      state.fieldsetBorderStyle,
      'solid',
      JSON.stringify(state, null, 2),
    );
    assert.equal(
      state.fieldsetBorderWidth,
      '2px',
      JSON.stringify(state, null, 2),
    );
    assert.ok(state.label, JSON.stringify(state, null, 2));
    assert.ok(
      (state.label?.y ?? Number.POSITIVE_INFINITY) <= (state.container?.y ?? 0),
      JSON.stringify(state, null, 2),
    );
    assert.equal(state.labelFontSize, '14px', JSON.stringify(state, null, 2));
    assert.equal(state.lineDisplay, 'none', JSON.stringify(state, null, 2));

    const evidencePath = process.env.CHIMERA_E2E_EVIDENCE_PATH;
    if (evidencePath) {
      fs.mkdirSync(path.dirname(evidencePath), { recursive: true });
      await browser.saveScreenshot(evidencePath);
    }
  });
});
