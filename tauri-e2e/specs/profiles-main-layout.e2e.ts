import assert from 'node:assert/strict';
import fs from 'node:fs';
import path from 'node:path';
import type { MutationOutcome } from '../../frontend/interface/src/ipc/bindings.js';
import { readAppMessageCatalog, xpathLiteral } from './app-message-catalog.js';
import { clickElement, displayedElement } from './interaction.js';
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
  items: Array<{ name: string; uid: string; type: 'config' | 'transform' }>;
  folders?: Array<{ id: string; name: string; members: string[] }>;
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

    const profileNameElement = await $(
      `//*[normalize-space()="${profileName}"]`,
    );
    await profileNameElement.waitForDisplayed({ timeout: 15_000 });
    const profiles = await invoke<ProfilesResponse>('get_profiles');
    profileUid = profiles.items.find((item) => item.name === profileName)?.uid;
    assert.ok(profileUid, 'The layout fixture profile was not persisted.');
  });

  // Each case starts from the real Profiles page, even if an earlier
  // assertion failed after navigating to the Inspect route.
  beforeEach(async () => {
    await openMainRoute(targetPath);
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

  // Ref navigation contract: groups are human-visible, each selectable link
  // changes the URL and selection without remounting the shared profile state.
  it('navigates between grouped profile types and restores All selection', async () => {
    await openMainRoute(targetPath);
    const navigation = await $('[data-slot="profiles-navigation"]');
    await navigation.waitForDisplayed({ timeout: 15_000 });

    const groups = await navigation.$$('[data-slot="navigation-list-group"]');
    assert.equal(
      groups.length,
      3,
      'Configs, Transforms and Pipeline groups should be present.',
    );

    const scripts = await navigation.$('a[href="/main/profiles/javascript"]');
    await scripts.waitForClickable({ timeout: 15_000 });
    await scripts.click();
    await browser.waitUntil(
      async () =>
        new URL(await browser.getUrl()).pathname ===
          '/main/profiles/javascript' &&
        (await scripts.getAttribute('aria-current')) === 'page',
      {
        timeout: 15_000,
        timeoutMsg:
          'Clicking JavaScript in the sidebar did not select its route.',
      },
    );

    const all = await navigation.$('a[href="/main/profiles/profile"]');
    await all.waitForClickable({ timeout: 15_000 });
    await all.click();
    await browser.waitUntil(
      async () =>
        new URL(await browser.getUrl()).pathname === targetPath &&
        (await all.getAttribute('aria-current')) === 'page',
      {
        timeout: 15_000,
        timeoutMsg:
          'All Profiles was not selected after returning from a transform.',
      },
    );

    assert.equal(
      await navigation.$('[data-slot="profiles-all-nav-item"]').isExisting(),
      true,
    );
    assert.equal(
      await navigation
        .$('[data-slot="global-transform-chain-nav"]')
        .isExisting(),
      true,
    );
  });

  // Contract: a user opens Inspect through the Profiles navigation. The
  // rendered content must agree with the read-only backend snapshot: either
  // show the actual step output or explicitly report that none exists.
  it('opens the runtime Inspect tool through the visible Profiles navigation', async () => {
    await openMainRoute(targetPath);
    const link = await $(
      '[data-slot="profiles-navigation"] a[href="/main/profiles/inspect"]',
    );
    await link.waitForClickable({ timeout: 15_000 });
    await link.click();

    await browser.waitUntil(
      async () =>
        new URL(await browser.getUrl()).pathname === '/main/profiles/inspect',
      {
        timeout: 15_000,
        timeoutMsg: 'Clicking Inspect did not open the Inspect route.',
      },
    );
    const page = await $('[data-slot="inspect-page"]');
    await page.waitForDisplayed({ timeout: 15_000 });
    const refresh = await $('[data-slot="inspect-refresh"]');
    await refresh.waitForDisplayed({ timeout: 15_000 });

    // Native Tauri invoke returns the Result's success payload directly:
    // a snapshot or null, not a { status, data } discriminated union.
    const snapshot = await invoke<{ snapshot_id: string } | null>(
      'inspect_runtime',
    );

    if (snapshot) {
      const details = await $('[data-slot="inspect-step-detail"]');
      await details.waitForDisplayed({ timeout: 15_000 });
      const outputTab = await $('[data-slot="inspect-tab-output"]');
      // Output is already selected for the final generated step. Do not
      // require a redundant click through an overlapping animated toolbar.
      if ((await outputTab.getAttribute('aria-selected')) !== 'true') {
        await outputTab.waitForClickable({ timeout: 15_000 });
        await outputTab.click();
      }
      assert.equal(await outputTab.getAttribute('aria-selected'), 'true');
      const yaml = await $('[data-slot="inspect-yaml-viewer"]');
      await yaml.waitForDisplayed({ timeout: 15_000 });
    } else {
      const empty = await $('[data-slot="inspect-empty"]');
      await empty.waitForDisplayed({ timeout: 15_000 });
    }

    // Switching snapshots is a visible interaction, not a direct cache or
    // route mutation. A core that has never been applied must show an empty
    // state rather than presenting the latest generated document as applied.
    const applied = await $('[data-slot="inspect-view-applied"]');
    await applied.waitForClickable({ timeout: 15_000 });
    await applied.click();
    await browser.waitUntil(
      async () => (await applied.getAttribute('data-state')) === 'on',
      {
        timeout: 15_000,
        timeoutMsg: 'The last-applied snapshot control was not selected.',
      },
    );
    const appliedSnapshot = await invoke<{ snapshot_id: string } | null>(
      'inspect_applied_runtime',
    );
    if (appliedSnapshot) {
      await $('[data-slot="inspect-step-detail"]').waitForDisplayed({
        timeout: 15_000,
      });
    } else {
      await $('[data-slot="inspect-empty"]').waitForDisplayed({
        timeout: 15_000,
      });
    }
    const latest = await $('[data-slot="inspect-view-latest"]');
    await latest.click();
    await browser.waitUntil(
      async () => (await latest.getAttribute('data-state')) === 'on',
      {
        timeout: 15_000,
        timeoutMsg: 'The generated snapshot control was not restored.',
      },
    );

    await openMainRoute(targetPath);
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

  // Contract: the folder dialog validates the name locally and cannot
  // mutate any Profile when the user enters only whitespace. This uses the
  // existing isolated E2E fixture and never creates persistent test folders.
  it('exposes a labeled folder dialog and rejects a blank name without writing', async () => {
    const before = await invoke<unknown>('get_profiles');
    const add = await $('[data-slot="profiles-folder-add"]');
    await add.waitForClickable({ timeout: 15_000 });
    assert.ok(await add.getAttribute('aria-label'));
    await add.click();

    const dialog = await $('[data-slot="folder-name-dialog"]');
    await dialog.waitForDisplayed({ timeout: 15_000 });
    const input = await dialog.$('input[aria-label]');
    await input.waitForDisplayed({ timeout: 15_000 });
    await input.setValue('   ');
    const save = await dialog.$('button[form="folder-name-dialog-form"]');
    await save.waitForClickable({ timeout: 15_000 });
    await save.click();
    assert.equal(await dialog.isDisplayed(), true);
    assert.deepEqual(
      await invoke<unknown>('get_profiles'),
      before,
      'Whitespace-only folder names must not change the durable document',
    );
    await browser.keys('Escape');
    await dialog.waitForDisplayed({ reverse: true, timeout: 15_000 });
  });

  // Contract: an FTP URL is syntactically valid but not a remote subscription.
  // The visible quick-import form must reject it without issuing a durable
  // Profile mutation. No external URL is fetched by this isolated test.
  it('rejects a non-HTTP quick-import URL without changing profiles', async () => {
    const before = await invoke<unknown>('get_profiles');
    const form = await $('[data-slot="profile-quick-import"]');
    await form.waitForDisplayed({ timeout: 15_000 });
    const input = await form.$('input[type="url"]');
    await input.waitForDisplayed({ timeout: 15_000 });
    const invalidUrl = 'ftp://example.invalid/not-a-subscription.yaml';
    await input.setValue(invalidUrl);

    const submit = await form.$('button[type="submit"]');
    await submit.waitForClickable({ timeout: 15_000 });
    await submit.click();

    assert.equal(
      await input.getValue(),
      invalidUrl,
      'Rejected URL must remain editable so the user can correct it',
    );
    assert.deepEqual(
      await invoke<unknown>('get_profiles'),
      before,
      'Non-HTTP quick import must not create or change any Profile',
    );
    const clear = await form.$('button[type="button"]');
    await clear.waitForClickable({ timeout: 15_000 });
    await clear.click();
    assert.equal(await input.getValue(), '');
  });

  // Contract: a person can right-click a Profile, move it to a newly named
  // folder, open the folder and use the card's details action. Read-only IPC
  // is used only to identify and restore this spec's isolated test data.
  it('creates a folder from the Profile menu and opens its visible member', async () => {
    assert.ok(profileUid);
    const before = await invoke<ProfilesResponse>('get_profiles');
    const previousFolders = before.folders ?? [];
    const assigned = new Set(
      previousFolders.flatMap((folder) => folder.members),
    );
    const originalLayout = {
      folders: previousFolders,
      ungrouped: before.items
        .filter((item) => item.type === 'config' && !assigned.has(item.uid))
        .map((item) => item.uid),
    };
    const folderName = `E2E Profile Folder ${Date.now()}`;
    try {
      // Open the same context menu a user uses, rather than creating a folder
      // through the backend IPC while bypassing the UI's query lifecycle.
      const originalCard = await $(
        `[data-slot="profile-card"][data-profile-uid="${profileUid}"]`,
      );
      await originalCard.waitForDisplayed({ timeout: 15_000 });
      await originalCard.click({ button: 'right' });

      const messages = await readAppMessageCatalog();
      const moveToFolder = messages.profile_move_to_folder;
      const newFolderLabel = messages.profile_move_new_folder;
      assert.ok(typeof moveToFolder === 'string' && moveToFolder.length > 0);
      assert.ok(
        typeof newFolderLabel === 'string' && newFolderLabel.length > 0,
      );
      const moveMenu = await $(
        `//*[@role="menuitem" and contains(normalize-space(.),${xpathLiteral(moveToFolder)})]`,
      );
      await moveMenu.waitForDisplayed({ timeout: 15_000 });
      await moveMenu.click();
      const newFolder = await $(
        `//*[@role="menuitem" and contains(normalize-space(.),${xpathLiteral(newFolderLabel)})]`,
      );
      await newFolder.waitForClickable({ timeout: 15_000 });
      await newFolder.click();

      const dialog = await $('[data-slot="folder-name-dialog"]');
      await dialog.waitForDisplayed({ timeout: 15_000 });
      const input = await dialog.$('input[aria-label]');
      await input.waitForDisplayed({ timeout: 15_000 });
      await input.setValue(folderName);
      const save = await dialog.$('button[form="folder-name-dialog-form"]');
      await save.waitForClickable({ timeout: 15_000 });
      await save.click();
      await dialog.waitForDisplayed({ reverse: true, timeout: 15_000 });

      const link = await $(`//a[starts-with(@aria-label,"${folderName}, ")]`);
      await link.waitForClickable({ timeout: 15_000 });
      const actions = await link
        .$('..')
        .$('[data-slot="profiles-folder-actions"]');
      await actions.waitForDisplayed({ timeout: 15_000 });
      await link.click();
      // The exiting page can still have an older folder header. Resolve the
      // one with the exact name that a user sees after opening this folder,
      // rather than asserting against the first match across both routes.
      const header = await $(
        `//header[@data-slot="profiles-folder-header"][.//h1[normalize-space(.)="${folderName}"]]`,
      );
      await header.waitForDisplayed({ timeout: 15_000 });
      // The folder route can remount after the layout mutation settles.
      // Never keep a link obtained from the header's parent: it can belong to
      // a detached, exiting page that WebDriver will click without navigating.
      // Query the current document by the accessible action and its real URL
      // immediately before the user click, and require exactly one visible hit.
      const detailsSelector = `a[aria-label][href="/main/profiles/profile/detail/${profileUid}"]`;
      let detailsLink: WebdriverIO.Element | undefined;
      // Check the folder-scoped import control before opening details:
      // clicking the details link must leave the folder page.
      let importVisible = false;
      for (const form of await $$('[data-slot="profile-quick-import"]')) {
        if (await form.isDisplayed()) {
          importVisible = true;
          break;
        }
      }
      assert.ok(
        importVisible,
        'A person must be able to use the folder-scoped import control',
      );
      try {
        // This is a real mouse click on a visible action, followed below
        // by checking the destination page. `isClickable()` can report false
        // for an animated card even when a person can click its details icon.
        // If WebDriver cannot click, clickElement reports the actual hit
        // target/obstruction instead of silently weakening the acceptance.
        // A user should be able to follow/copy this action's real destination.
        // Validate that navigation target before physically clicking it.
        detailsLink = await displayedElement(detailsSelector);
        const href = await detailsLink.getAttribute('href');
        assert.ok(href, 'The visible Profile details link must have a URL.');
        assert.equal(
          new URL(href, await browser.getUrl()).pathname,
          `/main/profiles/profile/detail/${profileUid}`,
          'The folder card details link must target the Profile detail page.',
        );
        await clickElement(detailsLink);
      } catch (cause) {
        // Capture the FAILED folder page before finally restores its layout;
        // afterTest screenshots otherwise show only the recovered main list.
        let screenshot = 'not captured';
        const artifactDir = process.env.CHIMERA_E2E_ARTIFACT_DIR;
        if (artifactDir) {
          const file = path.join(
            artifactDir,
            `profile-folder-visibility-${process.env.GITHUB_RUN_ID ?? 'local'}-${Date.now()}.png`,
          );
          try {
            fs.mkdirSync(artifactDir, { recursive: true });
            await browser.saveScreenshot(file);
            screenshot = file;
          } catch (error) {
            screenshot = `capture failed: ${String(error)}`;
          }
        }
        // Raw DOM is strictly failure-only evidence, never the acceptance
        // condition for this user interaction. A diagnostic failure must not
        // discard the original visibility failure.
        let state: unknown;
        try {
          state = await browser.execute(() => ({
            pathname: location.pathname,
            routes: Array.from(
              document.querySelectorAll<HTMLElement>('[data-route-present]'),
            ).map((route) => ({
              routeId: route.dataset.routeId ?? null,
              present: route.dataset.routePresent,
              opacity: getComputedStyle(route).opacity,
              cards: Array.from(
                route.querySelectorAll<HTMLElement>(
                  '[data-slot="profile-card"]',
                ),
              ).map((item) => ({
                uid: item.dataset.profileUid ?? null,
                displayed: getComputedStyle(item).display,
                visibility: getComputedStyle(item).visibility,
                bounds: {
                  width: item.getBoundingClientRect().width,
                  height: item.getBoundingClientRect().height,
                },
              })),
            })),
          }));
        } catch (diagnosticError) {
          state = { diagnosticError: String(diagnosticError) };
        }
        throw new Error(
          `Folder member details could not be clicked; cause: ${cause instanceof Error ? cause.message : String(cause)}; screenshot: ${screenshot}; route evidence: ${JSON.stringify(state)}`,
          { cause },
        );
      }
      const detailPath = `/main/profiles/profile/detail/${profileUid}`;
      try {
        await browser.waitUntil(
          async () => new URL(await browser.getUrl()).pathname === detailPath,
          {
            timeout: 15_000,
            timeoutMsg:
              'Clicking Profile details did not navigate to the detail URL.',
          },
        );
        const detail = await $(
          `[data-slot="profile-detail"][data-profile-uid="${profileUid}"]`,
        );
        await detail.waitForDisplayed({ timeout: 15_000 });
      } catch (cause) {
        // The outer catch screenshots this state before finally restores the
        // folder. Include the user-visible URL to distinguish a dead click
        // from a detail page that opened but failed to render.
        const observedUrl = await browser.getUrl().catch(() => 'unavailable');
        // Failure-only hit testing: find whether an exiting animated page is
        // still over the visible link. This is evidence, not a pass condition.
        let hitTest: unknown = 'unavailable';
        try {
          hitTest = await browser.execute(
            (link: HTMLElement, selector: string) => {
              const rect = link.getBoundingClientRect();
              const hit = document.elementFromPoint(
                rect.left + rect.width / 2,
                rect.top + rect.height / 2,
              );
              const route = (node: Element | null) =>
                node
                  ?.closest('[data-route-present]')
                  ?.getAttribute('data-route-present') ?? null;
              return {
                linkStillConnected: link.isConnected,
                liveMatchingLinks: document.querySelectorAll(selector).length,
                linkRoutePresent: route(link),
                centerHitTag: hit?.tagName ?? null,
                centerHitSlot:
                  (hit as HTMLElement | null)?.dataset.slot ?? null,
                centerHitRoutePresent: route(hit),
                linkReceivesHit:
                  hit === link || (hit !== null && link.contains(hit)),
                exitingRoutes: document.querySelectorAll(
                  '[data-route-present="false"]',
                ).length,
              };
            },
            await (detailsLink ?? $(detailsSelector)).getElement(),
            detailsSelector,
          );
        } catch (diagnosticError) {
          hitTest = `unavailable: ${String(diagnosticError)}`;
        }
        throw new Error(
          `The clicked Profile detail did not open; current URL: ${observedUrl}; pointer evidence: ${JSON.stringify(hitTest)}; cause: ${cause instanceof Error ? cause.message : String(cause)}`,
          { cause },
        );
      }
      assert.equal(
        new URL(await browser.getUrl()).pathname,
        `/main/profiles/profile/detail/${profileUid}`,
        'Opening folder card details must navigate to the Profile detail page',
      );
    } catch (cause) {
      // A failure at any earlier UI step (including the context menu) must
      // leave an image of that state, before the cleanup returns to the list.
      let screenshot = 'not captured';
      const artifactDir = process.env.CHIMERA_E2E_ARTIFACT_DIR;
      if (artifactDir) {
        const file = path.join(
          artifactDir,
          `profile-folder-flow-${process.env.GITHUB_RUN_ID ?? 'local'}-${Date.now()}.png`,
        );
        try {
          fs.mkdirSync(artifactDir, { recursive: true });
          await browser.saveScreenshot(file);
          screenshot = file;
        } catch (error) {
          screenshot = `capture failed: ${String(error)}`;
        }
      }
      // WebDriver's reporter does not expand Error.cause. Preserve the
      // original failure text in the top-level message as well as its cause.
      throw new Error(
        `Profile folder UI flow failed; screenshot: ${screenshot}; cause: ${cause instanceof Error ? `${cause.name}: ${cause.message}` : String(cause)}`,
        { cause },
      );
    } finally {
      // A response can be degraded or lost after the actor committed. Check
      // the document by the test-owned unique name before deciding cleanup.
      const persisted = await invoke<ProfilesResponse>('get_profiles');
      if (
        (persisted.folders ?? []).some((folder) => folder.name === folderName)
      ) {
        committedValue(
          await invoke<MutationOutcome<null>>('set_profile_layout', {
            layout: originalLayout,
          }),
          'Restore the original layout after folder route E2E',
        );
      }
      await openMainRoute(targetPath);
    }
  });

  // Contract: the visible Profiles navigation renders the correct icon and
  // material color for each type. Assert against the actual navigation item
  // structure, not obsolete data-profile-type-icon test-only attributes.
  it('renders the reference profile type icon compositions', async () => {
    const icons = await browser.execute(() => {
      const readIcon = (type: string, marker: string) => {
        const link = document.querySelector<HTMLElement>(
          `[data-slot="profiles-navigation"] a[href="/main/profiles/${type}"]`,
        );
        const icon = link?.querySelector<HTMLElement>(
          '[data-slot="navigation-list-item-icon"]',
        );
        const shape = icon?.querySelector<HTMLElement>(
          '[data-slot="material-shape"]',
        );
        return {
          hasPrimaryIcon: Boolean(icon?.querySelector('svg')),
          hasBadge: Boolean(shape),
          hasMarker: shape?.classList.contains(marker) ?? false,
        };
      };

      return {
        profile: readIcon('profile', 'bg-blue-container'),
        javascript: readIcon('javascript', 'bg-yellow-container'),
        lua: readIcon('lua', 'bg-cyan-container'),
        merge: readIcon('merge', 'bg-orange-container'),
      };
    });

    for (const [type, icon] of Object.entries(icons)) {
      assert.equal(icon.hasPrimaryIcon, true, `${type} is missing its icon.`);
      assert.equal(
        icon.hasBadge,
        true,
        `${type} is missing its material shape.`,
      );
      assert.equal(
        icon.hasMarker,
        true,
        `${type} does not use its visible reference color.`,
      );
    }
  });

  // Contract: the imported Profile card exposes the ref-aligned action
  // controls without allowing the active Profile to enter the sort gesture.
  // This asserts DOM affordances only; a real activation and drag remain
  // independent desktop interaction scenarios.
  it('exposes accessible card actions and guards active Profile dragging', async () => {
    assert.ok(profileUid);
    const card = await $(
      `[data-slot="profile-card"][data-profile-uid="${profileUid}"]`,
    );
    await card.waitForDisplayed({ timeout: 15_000 });
    const details = await card.$('[data-slot="profile-card-details"]');
    assert.equal(await details.isExisting(), true);
    assert.ok(await details.getAttribute('aria-label'));

    const sortable = await $(
      `[data-slot="profile-card-sortable"][data-profile-uid="${profileUid}"]`,
    );
    const isActive =
      (await card.getAttribute('data-profile-active')) === 'true';
    assert.equal(
      await sortable.getAttribute('data-drag-disabled'),
      String(isActive),
    );

    if (isActive) {
      assert.equal(
        await card.$('[data-slot="profile-card-gradient"]').isExisting(),
        true,
      );
    } else {
      const activate = await card.$('[data-slot="profile-card-activate"]');
      assert.equal(await activate.isExisting(), true);
      assert.ok(await activate.getAttribute('aria-label'));
    }
  });

  // Contract: the Profile list eventually displays every config returned by
  // the independent read-only get_profiles IPC, despite mounting six at a time.
  // No reload or extra write is allowed to make a missing card appear. The
  // existing test-owned fixture is sufficient; no extra Profiles are created.
  it('eventually renders every persisted config card after progressive batches', async () => {
    assert.ok(profileUid);
    const profiles = await invoke<ProfilesResponse>('get_profiles');
    const expected = profiles.items.filter(
      (item) => item.type === 'config',
    ).length;
    assert.ok(expected > 0, 'The synthetic Profile was not persisted.');

    let last = {
      total: 0,
      rendered: 0,
      cards: 0,
      foundFixture: false,
    };
    await browser.waitUntil(
      async () => {
        last = await browser.execute((uid) => {
          const grid = document.querySelector<HTMLElement>(
            '[data-slot="profiles-navigate"]',
          );
          const cards = grid?.querySelectorAll<HTMLElement>(
            '[data-slot="profile-card"][data-profile-uid]',
          );
          return {
            total: Number(grid?.dataset.totalCount ?? 0),
            rendered: Number(grid?.dataset.renderedCount ?? 0),
            cards: cards?.length ?? 0,
            foundFixture: Array.from(cards ?? []).some(
              (card) => card.dataset.profileUid === uid,
            ),
          };
        }, profileUid);
        return (
          last.total === expected &&
          last.rendered === expected &&
          last.cards === expected &&
          last.foundFixture
        );
      },
      {
        timeout: 15_000,
        interval: 200,
        timeoutMsg: () =>
          `Progressive Profile list did not settle: expected ${expected}, observed ${JSON.stringify(last)}`,
      },
    );
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

    // A visible card can still be displaced by Motion's layout spring after
    // moving between the folder and main Profile routes. In the Windows
    // failure snapshot, the sortable wrapper had translate3d(-11px, 0, 0)
    // while the test measured the card's left margin. Observe the actual
    // transform reaching rest; never change the layout tolerance or sleep.
    let lastCardFrame: unknown;
    try {
      await browser.waitUntil(
        async () => {
          const frame = await browser.execute(() => {
            const card = document.querySelector<HTMLElement>(
              '[data-slot="profile-card"]',
            );
            const sortable = card?.closest<HTMLElement>(
              '[data-slot="profile-card-sortable"]',
            );
            if (!card || !sortable) return null;
            const transform = getComputedStyle(sortable).transform;
            const matrix = new DOMMatrixReadOnly(
              transform === 'none' ? undefined : transform,
            );
            return {
              transform,
              left: card.getBoundingClientRect().left,
              settled:
                Math.abs(matrix.a - 1) < 0.0002 &&
                Math.abs(matrix.d - 1) < 0.0002 &&
                Math.abs(matrix.e) < 0.25 &&
                Math.abs(matrix.f) < 0.25,
            };
          });
          lastCardFrame = frame;
          return frame?.settled === true;
        },
        {
          timeout: 10_000,
          interval: 100,
          timeoutMsg: 'The Profile card layout animation did not settle.',
        },
      );
    } catch (cause) {
      throw new Error(
        `The Profile card layout animation did not settle: ${JSON.stringify(lastCardFrame)}`,
        { cause },
      );
    }

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
      // profiles-navigate is a flex section container, not the card grid.
      // Measure the real responsive grid containing the visible fixture card.
      const grid = card?.closest<HTMLElement>(
        '[data-slot="profiles-active"], [data-slot="profiles-grid"]',
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
        gridDisplay: getComputedStyle(grid).display,
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
    assert.equal(state.gridDisplay, 'grid', JSON.stringify(state, null, 2));
    assert.equal(
      state.gridColumns.split(' ').length >= 3,
      true,
      `Profile card grid column contract failed: ${JSON.stringify(state, null, 2)}`,
    );
    assert.equal(state.headerPosition, 'sticky');
    assert.equal(state.headerZIndex, '50');
    assert.equal(state.importRightGap >= 8 && state.importRightGap <= 48, true);
  });
});
