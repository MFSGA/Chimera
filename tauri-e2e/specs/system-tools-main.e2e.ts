import assert from 'node:assert/strict';
import fs from 'node:fs';
import path from 'node:path';
import { openMainRoute } from './main-window.js';

const targetPath = '/main/settings/system';

describe('main system tools reference layout', () => {
  before(async () => {
    await browser.setWindowSize(1240, 638);
    await browser.execute(() => {
      localStorage.setItem(btoa('paraglide-language-cache'), 'zh-cn');
    });
    await openMainRoute(targetPath);
    await browser.setWindowSize(1240, 638);

    if (process.platform === 'win32' || process.platform === 'darwin') {
      const tools = await $('[data-slot="system-tools-container"]');
      await tools.waitForDisplayed({ timeout: 15_000 });
    }
  });

  it('renders system tools supported by the current platform', async () => {
    const state = await browser.execute(() => {
      const tools = document.querySelector<HTMLElement>(
        '[data-slot="system-tools-container"]',
      );
      const card = document.querySelector<HTMLElement>(
        '[data-slot="uwp-tools-button-card"]',
      );
      const button = card?.querySelector<HTMLButtonElement>('button');
      const label = card?.querySelector<HTMLElement>(
        '[data-slot="settings-card-content-item-label-text"]',
      );
      const description = card?.querySelector<HTMLElement>(
        '[data-slot="settings-card-content-item-label-description"]',
      );
      const rect = (element: HTMLElement | null | undefined) =>
        element
          ? {
              width: Math.round(element.getBoundingClientRect().width),
              height: Math.round(element.getBoundingClientRect().height),
            }
          : null;

      return {
        tools: rect(tools),
        card: rect(card),
        button: rect(button),
        label: label?.textContent?.trim() ?? '',
        description: description?.textContent?.trim() ?? '',
        buttonTextAlign: button ? getComputedStyle(button).textAlign : null,
      };
    });

    if (process.platform === 'win32') {
      assert.ok(state.tools, JSON.stringify(state, null, 2));
      assert.ok(state.card, JSON.stringify(state, null, 2));
      assert.ok(state.button, JSON.stringify(state, null, 2));
      assert.ok(state.label.length > 0, JSON.stringify(state, null, 2));
      assert.ok(state.description.length > 0, JSON.stringify(state, null, 2));
      assert.equal(
        state.buttonTextAlign,
        'left',
        JSON.stringify(state, null, 2),
      );
      assert.ok(
        (state.button?.height ?? 0) >= 56,
        JSON.stringify(state, null, 2),
      );
    } else if (process.platform === 'darwin') {
      assert.ok(state.tools, JSON.stringify(state, null, 2));
      assert.equal(state.card, null, JSON.stringify(state, null, 2));
    } else {
      assert.equal(state.tools, null, JSON.stringify(state, null, 2));
      assert.equal(state.card, null, JSON.stringify(state, null, 2));
    }

    const evidencePath = process.env.CHIMERA_E2E_EVIDENCE_PATH;
    if (evidencePath) {
      fs.mkdirSync(path.dirname(evidencePath), { recursive: true });
      await browser.saveScreenshot(evidencePath);
    }
  });
});
