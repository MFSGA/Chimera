import assert from 'node:assert/strict';
import fs from 'node:fs';
import path from 'node:path';
import { openMainRoute } from './main-window.js';

describe('main global CSS contract', () => {
  it('scopes the ref baseline to main without changing legacy', async () => {
    await browser.setWindowSize(1240, 638);
    const handles = await browser.getWindowHandles();
    assert.ok(
      handles.includes('legacy'),
      'The legacy window is not available.',
    );
    await browser.switchToWindow('legacy');

    const legacy = await browser.execute(() => ({
      labelClass: document.documentElement.classList.contains('chimera-main'),
      fontFamily: getComputedStyle(document.body).fontFamily,
    }));

    assert.equal(legacy.labelClass, false, JSON.stringify(legacy, null, 2));

    await browser.execute(() => {
      localStorage.setItem(btoa('paraglide-language-cache'), 'zh-cn');
    });
    await openMainRoute('/main/dashboard');
    await browser.setWindowSize(1240, 638);

    const state = await browser.execute(() => {
      const rootStyle = getComputedStyle(document.documentElement);
      const bodyStyle = getComputedStyle(document.body);
      const probe = document.createElement('div');
      probe.style.cssText =
        'position:fixed;left:-9999px;top:-9999px;width:32px;height:32px;overflow:scroll';
      probe.append(document.createElement('div'));
      probe.firstElementChild?.setAttribute('style', 'width:64px;height:64px');
      document.body.append(probe);
      const scrollbarStyle = getComputedStyle(probe, '::-webkit-scrollbar');
      const result = {
        hasMainClass:
          document.documentElement.classList.contains('chimera-main'),
        hasThemeClass:
          document.documentElement.classList.contains('light') ||
          document.documentElement.classList.contains('dark'),
        cssContract: rootStyle
          .getPropertyValue('--chimera-main-css-contract')
          .trim(),
        mdBackground: rootStyle
          .getPropertyValue('--color-md-background')
          .trim(),
        fontFamily: bodyStyle.fontFamily,
        color: bodyStyle.color,
        backgroundColor: bodyStyle.backgroundColor,
        overflow: bodyStyle.overflow,
        userSelect: bodyStyle.userSelect,
        scrollbarWidth: scrollbarStyle.width,
      };
      probe.remove();
      return result;
    });

    assert.equal(state.hasMainClass, true, JSON.stringify(state, null, 2));
    assert.equal(state.hasThemeClass, true, JSON.stringify(state, null, 2));
    assert.equal(state.cssContract, '1', JSON.stringify(state, null, 2));
    assert.ok(state.mdBackground.length > 0, JSON.stringify(state, null, 2));
    assert.match(state.fontFamily, /system-ui|Segoe UI/i);
    assert.equal(state.overflow, 'hidden', JSON.stringify(state, null, 2));
    assert.equal(state.userSelect, 'none', JSON.stringify(state, null, 2));
    assert.notEqual(
      state.backgroundColor,
      'rgba(0, 0, 0, 0)',
      JSON.stringify(state, null, 2),
    );
    assert.equal(state.scrollbarWidth, '6px', JSON.stringify(state, null, 2));

    const evidencePath = process.env.CHIMERA_E2E_EVIDENCE_PATH;
    if (evidencePath) {
      fs.mkdirSync(path.dirname(evidencePath), { recursive: true });
      await browser.saveScreenshot(evidencePath);
    }
  });
});
