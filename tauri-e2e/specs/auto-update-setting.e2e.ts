import assert from 'node:assert/strict';
import type { UpdateSource } from '../../frontend/interface/src/ipc/bindings.js';
import { openMainRoute } from './main-window.js';
import { invoke, withCleanup } from './profile-fixtures.js';

const aboutPath = '/main/settings/about';
const legacySettingsPath = '/settings';
const switchSelector = '[data-slot="auto-check-updates-switch"][role="switch"]';

interface AppConfig {
  enable_auto_check_update: boolean;
  update_sources: UpdateSource[];
}

async function waitForApp() {
  await browser.waitUntil(
    async () =>
      browser.execute(
        () => (document.getElementById('root')?.childElementCount ?? 0) > 0,
      ),
    { timeout: 30_000, timeoutMsg: 'The Chimera frontend did not render.' },
  );
}

async function openAboutSettings() {
  await openMainRoute(aboutPath);
}

async function openLegacySettings() {
  await browser.switchToWindow('legacy');
  const currentHref = await browser.getUrl();
  await browser.url(new URL(legacySettingsPath, currentHref).href);
  await browser.waitUntil(
    async () =>
      browser.execute(
        (path) => window.location.pathname === path,
        legacySettingsPath,
      ),
    {
      timeout: 15_000,
      timeoutMsg: 'Legacy settings navigation did not complete.',
    },
  );
  await waitForApp();
  await waitForSourceControl('chimera');
}

async function getAutoUpdateSwitch() {
  const toggle = await $(switchSelector);
  await toggle.waitForClickable({ timeout: 15_000 });
  return toggle;
}

async function isChecked() {
  const toggle = await getAutoUpdateSwitch();
  return (await toggle.getAttribute('aria-checked')) === 'true';
}

async function setChecked(expected: boolean) {
  const toggle = await getAutoUpdateSwitch();
  const current = (await toggle.getAttribute('aria-checked')) === 'true';

  if (current !== expected) {
    await toggle.click();
  }

  await browser.waitUntil(async () => (await isChecked()) === expected, {
    timeout: 15_000,
    timeoutMsg: `Auto-update setting did not become ${String(expected)}.`,
  });
}

async function waitForSourceControl(source: UpdateSource) {
  await browser.waitUntil(
    async () =>
      (await $(`[data-slot="app-update-source-${source}"]`)).isDisplayed(),
    {
      timeout: 15_000,
      timeoutMsg: `Update source control ${source} did not appear.`,
    },
  );
}

async function isSourceSelected(source: UpdateSource) {
  await waitForSourceControl(source);
  const input = await $(`[data-slot="app-update-source-${source}"]`);
  return input.isSelected();
}

async function setSourceSelected(source: UpdateSource, expected: boolean) {
  const input = await $(`[data-slot="app-update-source-${source}"]`);
  await input.waitForDisplayed({ timeout: 15_000 });
  await input.waitForEnabled({ timeout: 15_000 });
  if ((await input.isSelected()) !== expected) await input.click();
  await browser.waitUntil(
    async () => (await isSourceSelected(source)) === expected,
    {
      timeout: 15_000,
      timeoutMsg: `Update source ${source} did not become ${String(expected)}.`,
    },
  );
}

async function readPersistedSources() {
  const config = await invoke<AppConfig>('get_app_config');
  assert.ok(Array.isArray(config.update_sources));
  return config.update_sources;
}

async function waitForPersistedSources(expected: UpdateSource[]) {
  let observed: UpdateSource[] = [];
  try {
    await browser.waitUntil(
      async () => {
        observed = await readPersistedSources();
        return JSON.stringify(observed) === JSON.stringify(expected);
      },
      {
        timeout: 15_000,
        interval: 250,
        timeoutMsg: `Persisted update sources did not become ${JSON.stringify(expected)}.`,
      },
    );
  } catch (cause) {
    throw new Error(
      `Persisted update sources differ; expected ${JSON.stringify(expected)}, observed ${JSON.stringify(observed)}.`,
      { cause },
    );
  }
}

// Source priority is meaningful: re-enabling a source appends it to the end.
// Restore the original order through the same visible move-up controls a user
// would use, rather than accepting a different persisted priority after cleanup.
async function restoreSourceOrder(expected: UpdateSource[]) {
  for (let targetIndex = 0; targetIndex < expected.length; targetIndex++) {
    const source = expected[targetIndex];
    let current = await readPersistedSources();
    assert.deepEqual(
      [...current].sort(),
      [...expected].sort(),
      'Restoring priorities must not add or remove update sources.',
    );

    while (current.indexOf(source) > targetIndex) {
      const oldIndex = current.indexOf(source);
      const moveUp = await $(
        `//input[@data-slot="app-update-source-${source}"]/../following-sibling::span/button[1]`,
      );
      await moveUp.waitForClickable({ timeout: 15_000 });
      await moveUp.click();
      await browser.waitUntil(
        async () =>
          (await readPersistedSources()).indexOf(source) === oldIndex - 1,
        {
          timeout: 15_000,
          timeoutMsg: `Update source ${source} did not move up in priority.`,
        },
      );
      current = await readPersistedSources();
    }
    assert.equal(
      current.indexOf(source),
      targetIndex,
      `Update source ${source} did not regain its original priority.`,
    );
  }
  await waitForPersistedSources(expected);
}

describe('Chimera auto-update preference', () => {
  it('persists the setting across a page reload and restores the original value', async () => {
    await waitForApp();
    await openAboutSettings();

    const original = await isChecked();
    const changed = !original;

    try {
      await setChecked(changed);
      await browser.refresh();
      await waitForApp();

      assert.equal(await isChecked(), changed);
    } finally {
      await setChecked(original);
      await browser.refresh();
      await waitForApp();
      assert.equal(await isChecked(), original);
    }
  });

  // Contract (T01/T02/T06/T14): with both WebViews mounted, clicking the
  // visible source control in one UI must persist the selected order and update
  // the other UI without reload. A missing typed commit or state event leaves
  // either the backend value or the other window's visible checkbox incorrect.
  // IPC is used only to read the authoritative typed setting; cleanup restores
  // the original value through the visible controls. This is not a cold-start
  // or package download/install test.
  it('shares update-source changes between the main and legacy settings UIs', async () => {
    await withCleanup('shared application update sources', async (defer) => {
      await openLegacySettings();
      const originalSources = await readPersistedSources();
      assert.ok(originalSources.length > 0);
      const source =
        originalSources.length > 1
          ? originalSources[0]
          : (['chimera', 'github', 'ghfast', 'sourceforge'] as const).find(
              (candidate) => !originalSources.includes(candidate),
            );
      assert.ok(
        source,
        'At least one source can be toggled without an empty set.',
      );
      const originalSelected = originalSources.includes(source);
      const changedSources = originalSelected
        ? originalSources.filter((item) => item !== source)
        : [...originalSources, source];

      await openAboutSettings();
      assert.equal(await isSourceSelected(source), originalSelected);

      defer(
        'restore update source selection through both visible UIs',
        async () => {
          await browser.switchToWindow('main');
          await setSourceSelected(source, originalSelected);
          await restoreSourceOrder(originalSources);
          await browser.switchToWindow('legacy');
          await setSourceSelected(source, originalSelected);
          await restoreSourceOrder(originalSources);
          await browser.switchToWindow('main');
          assert.equal(await isSourceSelected(source), originalSelected);
        },
      );

      await setSourceSelected(source, !originalSelected);
      await waitForPersistedSources(changedSources);

      await browser.switchToWindow('legacy');
      await browser.waitUntil(
        async () => (await isSourceSelected(source)) !== originalSelected,
        {
          timeout: 15_000,
          interval: 250,
          timeoutMsg:
            'Legacy update-source control did not receive the shared state change.',
        },
      );
      assert.deepEqual(await readPersistedSources(), changedSources);

      await setSourceSelected(source, originalSelected);
      await restoreSourceOrder(originalSources);
      await browser.switchToWindow('main');
      await browser.waitUntil(
        async () => (await isSourceSelected(source)) === originalSelected,
        {
          timeout: 15_000,
          interval: 250,
          timeoutMsg:
            'Main update-source control did not receive the restored state.',
        },
      );
    });
  });
});
