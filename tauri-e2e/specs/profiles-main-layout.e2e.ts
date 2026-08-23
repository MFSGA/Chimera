import assert from 'node:assert/strict';
import fs from 'node:fs';
import path from 'node:path';

const targetPath = '/main/profiles/profile';
const profileName = 'TDD Main Profile';

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

async function openMainWindow() {
  await invoke('create_main_window');
  await browser.waitUntil(
    async () => (await browser.getWindowHandles()).length > 1,
    {
      timeout: 15_000,
      timeoutMsg: 'The main window was not created.',
    },
  );

  for (const handle of await browser.getWindowHandles()) {
    await browser.switchToWindow(handle);
    const pathname = await browser.execute(() => location.pathname);
    if (pathname.startsWith('/main')) return;
  }

  throw new Error('The created main window could not be identified.');
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

describe('main profiles reference layout', () => {
  let profileUid: string | undefined;

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

    await openMainWindow();
    await browser.setWindowSize(1240, 638);

    const profilesLink = await $(`a[href="${targetPath}"]`);
    await profilesLink.waitForClickable({ timeout: 15_000 });
    await profilesLink.click();
    await waitForPath(targetPath);

    const importToggle = await $(
      '[data-slot="profile-import-button"] > div > button',
    );
    await importToggle.waitForClickable({ timeout: 15_000 });
    await importToggle.click();

    const localImport = await $(
      '[data-slot="profile-import-button"] > div > div button:nth-child(2)',
    );
    await localImport.waitForClickable({ timeout: 15_000 });
    await localImport.click();

    const nameInput = await $('input[name="name"]');
    await nameInput.waitForDisplayed({ timeout: 15_000 });
    await nameInput.setValue(profileName);
    const confirmButton = await $('//button[normalize-space()="OK"]');
    await confirmButton.waitForClickable({ timeout: 15_000 });
    await confirmButton.click();

    const profileNameElement = await $(
      `//*[normalize-space()="${profileName}"]`,
    );
    await profileNameElement.waitForDisplayed({ timeout: 15_000 });
    const profiles = await invoke<ProfilesResponse>('get_profiles');
    profileUid = profiles.items.find((item) => item.name === profileName)?.uid;
    assert.ok(profileUid, 'The layout fixture profile was not persisted.');
  });

  after(async () => {
    if (!profileUid) return;
    await invoke('activate_profile', { uid: null }).catch(() => undefined);
    await invoke('delete_profile', { uid: profileUid }).catch(() => undefined);
  });

  it('uses the reference tooltip surface for profile import actions', async () => {
    const importToggle = await $(
      '[data-slot="profile-import-button"] > div > button',
    );
    await importToggle.waitForClickable({ timeout: 15_000 });
    await importToggle.click();

    const remoteImport = await $(
      '[data-slot="profile-import-button"] > div > div button:first-child',
    );
    await remoteImport.waitForDisplayed({ timeout: 15_000 });
    await browser.execute((element) => {
      element.dispatchEvent(
        new PointerEvent('pointermove', {
          bubbles: true,
          pointerType: 'mouse',
        }),
      );
    }, remoteImport);

    const tooltip = await $('[role="tooltip"]');
    await tooltip.waitForDisplayed({ timeout: 15_000 });
    assert.equal((await tooltip.getText()).includes('远程配置'), true);

    await importToggle.click();
  });

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

  it('matches the reference desktop structure and remains visually balanced', async () => {
    const card = await $('[data-slot="profile-card"]');
    await card.waitForDisplayed({ timeout: 15_000 });
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
