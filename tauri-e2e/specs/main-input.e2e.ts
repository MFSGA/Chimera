import assert from 'node:assert/strict';
import fs from 'node:fs';
import path from 'node:path';
import { displayedElement } from './interaction.js';
import { openMainRoute } from './main-window.js';

const targetPath = '/main/settings/web-ui';

describe('main ref input primitive', () => {
  before(async () => {
    await browser.setWindowSize(1240, 638);
    await browser.execute(() => {
      localStorage.setItem(btoa('paraglide-language-cache'), 'zh-cn');
    });
    await openMainRoute(targetPath);
    await browser.setWindowSize(1240, 638);
  });

  it('uses the ref outlined fieldset, floating label, and hidden filled line', async () => {
    const cardSelector = '[data-slot="external-controller-config-card"]';
    const triggerSelector = `${cardSelector} [data-slot="modal-trigger"]`;
    await browser.waitUntil(
      async () => {
        const card = await $(cardSelector);
        const trigger = await $(triggerSelector);
        return (await card.isDisplayed()) && (await trigger.isDisplayed());
      },
      {
        timeout: 15_000,
        timeoutMsg: 'The external controller modal trigger did not appear.',
      },
    );

    const trigger = await $(triggerSelector);
    const placeholderContainer = await trigger.$(
      '[data-slot="modal-trigger-placeholder-container"]',
    );
    const placeholder = await trigger.$(
      '[data-slot="modal-trigger-placeholder"]',
    );
    assert.equal(await placeholderContainer.isExisting(), true);
    assert.equal(await placeholder.isExisting(), true);
    await trigger.click();

    const modal = await displayedElement('[data-slot="modal-content"]');
    const input = await modal.$('input');
    await input.waitForDisplayed({ timeout: 15_000 });
    await input.click();
    await browser.waitUntil(
      async () =>
        browser.execute((modalNode) => {
          const modal = modalNode as HTMLElement;
          const input = modal.querySelector<HTMLInputElement>('input');
          const container = input?.parentElement ?? null;
          const label = container?.querySelector<HTMLElement>('label') ?? null;
          return Boolean(
            input &&
            container &&
            label &&
            document.activeElement === input &&
            label.getBoundingClientRect().y <=
              container.getBoundingClientRect().y,
          );
        }, modal),
      {
        timeout: 5_000,
        timeoutMsg: 'The focused input label did not reach its floating state.',
      },
    );

    const state = await browser.execute((modalNode) => {
      const modal = modalNode as HTMLElement;
      const input = modal.querySelector<HTMLInputElement>('input');
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
    }, modal);

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
