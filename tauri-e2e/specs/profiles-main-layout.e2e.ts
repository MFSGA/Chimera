import assert from 'node:assert/strict';
import fs from 'node:fs';
import path from 'node:path';
import type { MutationOutcome } from '../../frontend/interface/src/ipc/bindings.js';
import { openMainRoute } from './main-window.js';
import {
  appliedValue,
  committedValue,
  runCleanupActions,
} from './profile-fixtures.js';

const targetPath = '/main/profiles/profile';
const profileName = `TDD Main Profile ${Date.now()}`;

type ProfilesResponse = {
  current: string | null;
  items: Array<{ name: string; uid: string }>;
};

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

describe('main profiles reference layout', () => {
  let profileUid: string | undefined;
  let previousCurrent: string | null = null;
  let selectionCaptured = false;

  before(async () => {
    await browser.setWindowSize(1240, 638);
    await browser.waitUntil(
      async () =>
        browser.execute(
          () =>
            document.readyState === 'complete' &&
            (document.getElementById('root')?.childElementCount ?? 0) > 0,
        ),
      { timeout: 30_000, timeoutMsg: 'The Chimera frontend did not render.' },
    );
    await browser.execute(() => {
      localStorage.setItem(btoa('paraglide-language-cache'), 'zh-cn');
    });

    await openMainRoute(targetPath);
    await browser.setWindowSize(1240, 638);

    previousCurrent = (await invoke<ProfilesResponse>('get_profiles')).current;
    selectionCaptured = true;

    const importToggle = await $('[data-slot="profile-import-toggle"]');
    await importToggle.waitForClickable({ timeout: 15_000 });
    await importToggle.click();

    const localImport = await $('[data-slot="profile-import-local-action"]');
    await localImport.waitForClickable({ timeout: 15_000 });
    await localImport.click();

    const nameInput = await $('input[name="name"]');
    await nameInput.waitForDisplayed({ timeout: 15_000 });
    await nameInput.setValue(profileName);
    const confirmButton = await $('//button[normalize-space()="OK"]');
    await confirmButton.waitForClickable({ timeout: 15_000 });
    await confirmButton.click();

    const profiles = await invoke<ProfilesResponse>('get_profiles');
    const createdProfile = profiles.items.find(
      (item) => item.name === profileName,
    );
    assert.ok(createdProfile, 'The layout fixture profile was not persisted.');
    profileUid = createdProfile.uid;

    const profileCard = await $(
      `[data-slot="profile-card"][data-profile-uid="${profileUid}"]`,
    );
    await profileCard.waitForDisplayed({ timeout: 15_000 });
  });

  after(async () => {
    const uid =
      profileUid ??
      (await invoke<ProfilesResponse>('get_profiles')).items.find(
        (item) => item.name === profileName,
      )?.uid;
    if (!uid) return;
    await runCleanupActions('main profiles layout spec', [
      {
        label: 'restore the original Profile selection',
        run: async () => {
          if (!selectionCaptured) return;
          const profiles = await invoke<ProfilesResponse>('get_profiles');
          if (
            profiles.current !== uid ||
            profiles.current === previousCurrent
          ) {
            return;
          }
          appliedValue(
            await invoke<MutationOutcome<null>>('activate_profile', {
              uid: previousCurrent,
            }),
            'Profile selection restoration',
          );
          await browser.waitUntil(
            async () =>
              (await invoke<ProfilesResponse>('get_profiles')).current ===
              previousCurrent,
            {
              timeout: 30_000,
              timeoutMsg: 'The original Profile selection was not restored.',
            },
          );
        },
      },
      {
        label: 'delete the layout test Profile',
        run: async () => {
          const profiles = await invoke<ProfilesResponse>('get_profiles');
          assert.notEqual(
            profiles.current,
            uid,
            'The layout test Profile is active and cannot be deleted safely.',
          );
          committedValue(
            await invoke<MutationOutcome<null>>('delete_profile', { uid }),
            'Layout test Profile deletion',
          );
          await browser.waitUntil(
            async () =>
              !(await invoke<ProfilesResponse>('get_profiles')).items.some(
                (item) => item.uid === uid,
              ),
            {
              timeout: 30_000,
              timeoutMsg: 'The layout test Profile was not removed.',
            },
          );
        },
      },
    ]);
  });

  // Contract: from the main Profile list, opening the create menu exposes a
  // visible remote action with its localized accessible name. The bounded
  // hover limitation is recorded in docs/testing/contracts/profile-import-accessible-action.md.
  it('exposes the remote profile import action with an accessible name', async () => {
    const importToggle = await $('[data-slot="profile-import-toggle"]');
    await importToggle.waitForClickable({ timeout: 15_000 });
    await importToggle.click();

    const remoteImport = await $('[data-slot="profile-import-remote-action"]');
    await remoteImport.waitForDisplayed({ timeout: 15_000 });

    const remoteLabel = await remoteImport.getAttribute('aria-label');
    assert.ok(
      remoteLabel,
      'Remote import action must expose an accessible label.',
    );
    await importToggle.click();
  });

  // Contract: the persisted local Profile, overlay and JS transform use the
  // expected icon compositions and badges. DOM icon slots and classes provide
  // visual evidence; a missing type mapping fails its own assertion.
  it('uses the reference profile type icon compositions', async () => {
    const icons = await browser.execute(() => {
      const readIcon = (type: string, marker: string) => {
        const icon = document.querySelector<HTMLElement>(
          `[data-profile-type-icon="${type}"]`,
        );
        const badge = icon?.querySelector<HTMLElement>(
          `[data-profile-type-badge="${type}"]`,
        );
        return {
          hasPrimaryIcon: (icon?.querySelectorAll('svg').length ?? 0) > 0,
          hasBadge: Boolean(badge),
          hasMarker: (badge?.getAttribute('class') ?? '').includes(marker),
        };
      };

      return {
        profile: readIcon('profile', 'bg-gray-300'),
        javascript: readIcon('javascript', 'bg-amber-400'),
        lua: readIcon('lua', 'bg-blue-300'),
        merge: readIcon('merge', 'bg-orange-400'),
      };
    });

    for (const [type, icon] of Object.entries(icons)) {
      assert.equal(
        icon.hasPrimaryIcon,
        true,
        `${type} is missing its primary icon.`,
      );
      assert.equal(
        icon.hasBadge,
        true,
        `${type} is missing its reference badge.`,
      );
      assert.equal(
        icon.hasMarker,
        true,
        `${type} badge style is not ref-aligned.`,
      );
    }
  });

  // Contract: at the fixed 1240x638 main-window viewport, Profile sidebar,
  // list, card, header and import action remain within measured layout bounds.
  // Missing elements or a geometry regression fails these assertions.
  it('matches the reference desktop structure and remains visually balanced', async () => {
    const card = await $('[data-slot="profile-card"]');
    await card.waitForDisplayed({ timeout: 15_000 });
    await browser.execute(async () => {
      await document.fonts.ready;
    });
    const evidencePath = process.env.CHIMERA_E2E_EVIDENCE_PATH;
    if (evidencePath) {
      fs.mkdirSync(path.dirname(evidencePath), { recursive: true });
      await browser.saveScreenshot(evidencePath);
    }

    const state = await browser.execute(() => {
      const sidebar = document.querySelector<HTMLElement>(
        '[data-slot="profiles-sidebar-scroll-area"]',
      );
      const content = document.querySelector<HTMLElement>(
        '[data-slot="profiles-content-scroll-area"]',
      );
      const list = document.querySelector<HTMLElement>(
        '[data-slot="profiles-list"]',
      );
      const card = document.querySelector<HTMLElement>(
        '[data-slot="profile-card"]',
      );
      const grid = document.querySelector<HTMLElement>(
        '[data-slot="profiles-navigate"]',
      );
      const header = document.querySelector<HTMLElement>(
        '[data-slot="profiles-header"]',
      );
      const quickImport = document.querySelector<HTMLElement>(
        '[data-slot="profiles-header"] form',
      );
      const importButton = document.querySelector<HTMLElement>(
        '[data-slot="profile-import-button"]',
      );
      if (
        !sidebar ||
        !content ||
        !list ||
        !card ||
        !grid ||
        !header ||
        !quickImport ||
        !importButton
      ) {
        return { missing: true } as const;
      }
      const sidebarRect = sidebar.getBoundingClientRect();
      const contentRect = content.getBoundingClientRect();
      const cardRect = card.getBoundingClientRect();
      const importRect = importButton.getBoundingClientRect();
      return {
        missing: false as const,
        sidebarWidth: sidebarRect.width,
        contentWidth: contentRect.width,
        cardWidth: cardRect.width,
        cardHeight: cardRect.height,
        cardLeft: cardRect.left,
        contentLeft: contentRect.left,
        gridColumns: getComputedStyle(grid).gridTemplateColumns,
        headerPosition: getComputedStyle(header).position,
        headerZIndex: getComputedStyle(header).zIndex,
        importRightGap: window.innerWidth - importRect.right,
        viewport: { width: window.innerWidth, height: window.innerHeight },
      };
    });

    assert.equal(state.missing, false, JSON.stringify(state, null, 2));
    if (state.missing) return;
    assert.equal(state.viewport.width >= 1200, true);
    assert.equal(state.sidebarWidth >= 180 && state.sidebarWidth <= 360, true);
    assert.equal(state.contentWidth > state.sidebarWidth * 2, true);
    assert.equal(state.cardWidth >= 220 && state.cardWidth <= 420, true);
    assert.equal(state.cardHeight >= 150 && state.cardHeight <= 260, true);
    assert.equal(state.cardLeft >= state.contentLeft + 8, true);
    assert.equal(state.gridColumns.split(' ').length >= 3, true);
    assert.equal(state.headerPosition, 'sticky');
    assert.equal(state.headerZIndex, '50');
    assert.equal(state.importRightGap >= 8 && state.importRightGap <= 48, true);
  });
});
