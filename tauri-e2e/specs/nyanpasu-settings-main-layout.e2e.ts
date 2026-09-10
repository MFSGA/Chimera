import assert from 'node:assert/strict';
import fs from 'node:fs';
import path from 'node:path';
import { openMainRoute } from './main-window.js';

describe('main Chimera settings reference layout', () => {
  before(async () => {
    await browser.setWindowSize(1240, 638);
    await browser.execute(() => {
      localStorage.setItem(btoa('paraglide-language-cache'), 'zh-cn');
    });
    await openMainRoute('/main/settings/chimera');
    await browser.setWindowSize(1240, 638);
  });

  it('matches the ref settings-group DOM and spacing contract', async () => {
    const firstGroup = await $('[data-slot="app-settings-container"]');
    await firstGroup.waitForDisplayed({ timeout: 15_000 });

    const state = await browser.execute(() => {
      const groups = Array.from(
        document.querySelectorAll<HTMLElement>(
          '[data-slot="app-settings-container"]',
        ),
      );
      const title = document.querySelector<HTMLElement>(
        '[data-slot="settings-title"]',
      );
      const content = groups[0]?.parentElement;
      const labels = groups.map((group) =>
        group.querySelector<HTMLElement>('[data-slot="settings-label"]'),
      );
      const settingsGroups = groups.map((group) =>
        group.querySelector<HTMLElement>('[data-slot="settings-group"]'),
      );

      if (
        !title ||
        !content ||
        groups.length !== 4 ||
        labels.some((label) => !label) ||
        settingsGroups.some((group) => !group)
      ) {
        throw new Error('Chimera settings reference DOM is incomplete.');
      }

      const titleStyle = getComputedStyle(title);
      const contentStyle = getComputedStyle(content);
      const firstLabelStyle = getComputedStyle(labels[0]!);
      const firstSettingsGroupStyle = getComputedStyle(settingsGroups[0]!);
      const firstGroupRect = groups[0]!.getBoundingClientRect();
      const secondGroupRect = groups[1]!.getBoundingClientRect();

      return {
        path: location.pathname,
        viewport: { width: innerWidth, height: innerHeight },
        groupCount: groups.length,
        groupTags: groups.map((group) => group.tagName),
        directSlots: groups.map((group) =>
          Array.from(group.children).map((child) =>
            child.getAttribute('data-slot'),
          ),
        ),
        title: {
          height: titleStyle.height,
          position: titleStyle.position,
          top: titleStyle.top,
          zIndex: titleStyle.zIndex,
        },
        content: {
          paddingLeft: contentStyle.paddingLeft,
          paddingRight: contentStyle.paddingRight,
          paddingBottom: contentStyle.paddingBottom,
        },
        groupSpacing: {
          gap: secondGroupRect.top - firstGroupRect.bottom,
        },
        label: {
          fontSize: firstLabelStyle.fontSize,
          lineHeight: firstLabelStyle.lineHeight,
        },
        settingsGroup: {
          display: firstSettingsGroupStyle.display,
          rowGap: firstSettingsGroupStyle.rowGap,
        },
      };
    });

    assert.equal(state.path, '/main/settings/chimera');
    assert.deepEqual(state.viewport, { width: 1224, height: 629 });
    assert.equal(state.groupCount, 4);
    assert.deepEqual(state.groupTags, ['DIV', 'DIV', 'DIV', 'DIV']);
    assert.deepEqual(
      state.directSlots,
      Array.from({ length: 4 }, () => ['settings-label', 'settings-group']),
    );
    assert.deepEqual(state.title, {
      height: '64px',
      position: 'sticky',
      top: '0px',
      zIndex: '10',
    });
    assert.deepEqual(state.content, {
      paddingLeft: '16px',
      paddingRight: '16px',
      paddingBottom: '16px',
    });
    assert.deepEqual(state.groupSpacing, {
      gap: 16,
    });
    assert.match(state.label.fontSize, /^\d+(?:\.\d+)?px$/);
    assert.match(state.label.lineHeight, /^\d+(?:\.\d+)?px$/);
    assert.equal(state.settingsGroup.display, 'flex');
    assert.equal(state.settingsGroup.rowGap, '4px');

    const evidencePath = process.env.CHIMERA_E2E_EVIDENCE_PATH;
    if (evidencePath) {
      fs.mkdirSync(path.dirname(evidencePath), { recursive: true });
      await browser.saveScreenshot(evidencePath);
    }
  });
});
