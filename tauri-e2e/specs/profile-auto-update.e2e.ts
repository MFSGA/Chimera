import assert from 'node:assert/strict';
import fs from 'node:fs';
import { createServer, type Server } from 'node:http';
import path from 'node:path';
import type {
  MutationOutcome,
  ProfileDocument_Deserialize,
  ProfileSyncStatus,
  RunPageDto,
} from '../../frontend/interface/src/ipc/bindings.js';
import { ensureMainWindow } from './main-window.js';
import {
  committedValue,
  invoke,
  readProfiles,
  withCleanup,
} from './profile-fixtures.js';

const fixtureProfileName = `Auto Update E2E ${process.pid}-${Date.now()}`;
// Contract: docs/testing/contracts/profile-sync-history.md
const screenshotRunId = process.env.GITHUB_RUN_ID
  ? `${process.env.GITHUB_RUN_ID}-${process.env.GITHUB_RUN_ATTEMPT ?? '1'}`
  : `local-${process.pid}-${Date.now()}`;
let screenshotAttempt = 0;

type VergeConfig = {
  enable_system_proxy?: boolean | null;
  enable_tun_mode?: boolean | null;
};

function remoteUpdateInterval(
  profiles: ProfileDocument_Deserialize,
  uid: string,
): number {
  const item = profiles.items.find((candidate) => candidate.uid === uid);
  assert.ok(item, `Profile ${uid} was not persisted.`);
  assert.equal(item.type, 'config');
  if (item.type !== 'config' || item.config.type !== 'file') {
    throw new Error(`Profile ${uid} is not a file-backed config.`);
  }
  assert.equal(item.config.source.type, 'remote');
  if (item.config.source.type !== 'remote') {
    throw new Error(`Profile ${uid} is not remote.`);
  }
  return item.config.source.option?.update_interval_minutes ?? 120;
}

function persistedUpdateInterval(uid: string): number {
  const runtimeRoot = process.env.CHIMERA_E2E_RUNTIME_DIR;
  assert.ok(runtimeRoot, 'CHIMERA_E2E_RUNTIME_DIR is not configured.');
  const profilesPath = path.join(runtimeRoot, 'config', 'profiles.yaml');
  const contents = fs.readFileSync(profilesPath, 'utf8');
  const escapedUid = uid.replace(/[.*+?^${}()|[\]\\]/g, '\\$&');
  const profile = contents
    .split(/(?=^[ \t]*-[ \t]+uid:[ \t]*)/m)
    .find((entry) =>
      new RegExp(`^[ \\t]*-[ \\t]+uid:[ \\t]*${escapedUid}[ \\t]*$`, 'm').test(
        entry,
      ),
    );
  assert.ok(profile, `Profile ${uid} is missing from persisted profiles.yaml.`);
  const interval = profile.match(
    /^[ \t]+update_interval_minutes:[ \t]*(\d+)[ \t]*$/m,
  );
  assert.ok(interval, `Profile ${uid} has no persisted update interval.`);
  return Number(interval[1]);
}

async function closeServer(server: Server) {
  if (!server.listening) return;
  await new Promise<void>((resolve, reject) =>
    server.close((error) => (error ? reject(error) : resolve())),
  );
}

async function waitForInterval(uid: string, expected: number) {
  let lastObserved: number | undefined;
  try {
    await browser.waitUntil(
      async () => {
        lastObserved = remoteUpdateInterval(await readProfiles(), uid);
        return lastObserved === expected;
      },
      {
        timeout: 15_000,
        timeoutMsg: `The saved auto-update interval did not become ${expected}.`,
      },
    );
  } catch (cause) {
    throw new Error(
      `The saved auto-update interval did not become ${expected}; last observed ${String(lastObserved)}.`,
      { cause },
    );
  }
}

async function waitForPersistedInterval(uid: string, expected: number) {
  let lastObserved: number | undefined;
  try {
    await browser.waitUntil(
      async () => {
        lastObserved = persistedUpdateInterval(uid);
        return lastObserved === expected;
      },
      {
        timeout: 15_000,
        timeoutMsg: `profiles.yaml did not persist update interval ${expected}.`,
      },
    );
  } catch (cause) {
    throw new Error(
      `profiles.yaml did not persist interval ${expected}; last observed ${String(lastObserved)}.`,
      { cause },
    );
  }
}

async function waitForSuccessfulSyncHistory(uid: string) {
  let lastObserved = 'not observed';
  try {
    await browser.waitUntil(
      async () => {
        const state = await browser.execute((profileUid) => {
          const card = document.querySelector<HTMLElement>(
            `[data-slot="profile-sync-history"][data-profile-uid="${profileUid}"]`,
          );
          const successfulRun = card?.querySelector<HTMLElement>(
            '[data-slot="profile-sync-run"][data-run-status="succeeded"]',
          );
          const isDisplayed = (element: HTMLElement | null | undefined) => {
            if (!element) return false;
            const style = getComputedStyle(element);
            const rect = element.getBoundingClientRect();
            return (
              rect.width > 0 &&
              rect.height > 0 &&
              style.display !== 'none' &&
              style.visibility !== 'hidden' &&
              style.opacity !== '0'
            );
          };
          return {
            cardVisible: isDisplayed(card),
            successfulRunVisible: isDisplayed(successfulRun),
            renderedLogCount:
              successfulRun?.querySelectorAll(
                '[data-slot="profile-sync-logs"] li',
              ).length ?? 0,
          };
        }, uid);
        lastObserved = JSON.stringify(state);
        return (
          state.cardVisible &&
          state.successfulRunVisible &&
          state.renderedLogCount > 0
        );
      },
      {
        timeout: 30_000,
        timeoutMsg: `Profile ${uid} did not show a completed sync with logs.`,
      },
    );
  } catch (cause) {
    throw new Error(
      `Profile ${uid} did not show a completed sync with logs; last observed ${lastObserved}.`,
      { cause },
    );
  }
}

async function waitForExternalManualSync(
  uid: string,
  previousSequence: string | null,
) {
  let lastObserved = 'not observed';
  try {
    await browser.waitUntil(
      async () => {
        const status = await invoke<ProfileSyncStatus>(
          'get_profile_sync_status',
          { uid },
        );
        if (status.latest_run_sequence === previousSequence) {
          lastObserved = `sequence unchanged at ${String(previousSequence)}`;
          return false;
        }
        const page = await invoke<RunPageDto>('get_profile_sync_runs', {
          uid,
          after: null,
        });
        const run = page.items.find(
          (item) => item.admission_sequence === status.latest_run_sequence,
        );
        lastObserved = JSON.stringify({
          latestRunSequence: status.latest_run_sequence,
          latestRunState: run?.state.kind,
          latestRunTrigger: run?.trigger,
        });
        if (!run || run.trigger !== 'manual' || run.state.kind !== 'finished') {
          return false;
        }
        return run.state.completion.outcome.kind === 'succeeded';
      },
      {
        timeout: 30_000,
        timeoutMsg: `External manual sync for Profile ${uid} did not finish.`,
      },
    );
  } catch (cause) {
    throw new Error(
      `External manual sync for Profile ${uid} did not finish; last observed ${lastObserved}.`,
      { cause },
    );
  }
}

async function waitForSwitchChecked(selector: string, expected: boolean) {
  let lastObserved = 'not observed';
  try {
    await browser.waitUntil(
      async () => {
        const state = await browser.execute((target) => {
          const switchElement = document.querySelector<HTMLElement>(target);
          const switches = Array.from(
            document.querySelectorAll<HTMLElement>(
              '[data-slot="profile-auto-update-switch"]',
            ),
          );

          return {
            url: location.href,
            visibility: document.visibilityState,
            focused: document.hasFocus(),
            modalStates: Array.from(
              document.querySelectorAll<HTMLElement>(
                '[data-slot="modal-content"]',
              ),
              (modal) => modal.getAttribute('data-state'),
            ),
            targetChecked: switchElement?.getAttribute('aria-checked') ?? null,
            switches: switches.map((element) => ({
              checked: element.getAttribute('aria-checked'),
              state: element.getAttribute('data-state'),
              connected: element.isConnected,
            })),
            activeElement:
              document.activeElement instanceof HTMLElement
                ? {
                    tagName: document.activeElement.tagName,
                    slot: document.activeElement.dataset.slot ?? null,
                  }
                : null,
          };
        }, selector);
        lastObserved = JSON.stringify(state);
        return state.targetChecked === String(expected);
      },
      {
        timeout: 10_000,
        timeoutMsg: `The Auto Update switch did not become ${String(expected)}.`,
      },
    );
  } catch (cause) {
    let screenshotEvidence = '';
    const artifactDirectory = process.env.CHIMERA_E2E_ARTIFACT_DIR;
    if (artifactDirectory) {
      const screenshotPath = path.join(
        artifactDirectory,
        screenshotRunId,
        'profile-auto-update.e2e',
        'disables-scheduled-sync-while-keeping-manual-sync-available-in-both-uis',
        `profile-auto-update-switch-state-attempt-${++screenshotAttempt}.png`,
      );
      try {
        fs.mkdirSync(path.dirname(screenshotPath), { recursive: true });
        await browser.saveScreenshot(screenshotPath);
        screenshotEvidence = ` Failure screenshot: ${screenshotPath}.`;
      } catch (screenshotError) {
        screenshotEvidence = ` Screenshot capture failed: ${String(screenshotError)}.`;
      }
    }
    throw new Error(
      `The Auto Update switch did not become ${String(expected)}; last observed ${lastObserved}.${screenshotEvidence}`,
      { cause },
    );
  }
}

async function saveMainOptions() {
  const saveButton = await $('[data-slot="profile-update-options-save"]');
  await saveButton.waitForExist({ timeout: 15_000 });
  await saveButton.click();
  await browser.waitUntil(
    async () =>
      browser.execute(
        () =>
          document.querySelector(
            '[data-slot="modal-content"][data-state="open"]',
          ) === null,
      ),
    {
      timeout: 15_000,
      timeoutMsg: 'The update-options dialog did not enter its closed state.',
    },
  );
}

async function openMainUpdateOptions(uid: string) {
  const trigger = await $(
    `[data-slot="profile-update-options-trigger"][data-profile-uid="${uid}"]`,
  );
  try {
    await trigger.waitForDisplayed({ timeout: 15_000 });
  } catch (cause) {
    throw new Error(
      `The update-options trigger for Profile ${uid} was not displayed. ${await captureMainProfileState(uid)}`,
      { cause },
    );
  }
  await trigger.click();

  let lastObserved = 'not observed';
  try {
    await browser.waitUntil(
      async () => {
        const state = await browser.execute(
          (expectedUid) => ({
            url: location.href,
            profileDetails: Array.from(
              document.querySelectorAll<HTMLElement>(
                '[data-slot="profile-detail"]',
              ),
            ).map((detail) => ({
              uid: detail.dataset.profileUid ?? null,
              opacity: getComputedStyle(detail).opacity,
              text: detail.innerText.slice(0, 180),
            })),
            triggerCount: document.querySelectorAll(
              `[data-slot="profile-update-options-trigger"][data-profile-uid="${expectedUid}"]`,
            ).length,
            modalStates: Array.from(
              document.querySelectorAll<HTMLElement>(
                '[data-slot="modal-content"]',
              ),
              (modal) => modal.getAttribute('data-state'),
            ),
            activeSlot:
              (document.activeElement as HTMLElement | null)?.dataset.slot ??
              null,
          }),
          uid,
        );
        lastObserved = JSON.stringify(state);
        return state.modalStates.includes('open');
      },
      {
        timeout: 15_000,
        timeoutMsg: 'The main Profile update-options dialog did not open.',
      },
    );
  } catch (cause) {
    let screenshotEvidence = '';
    const artifactDirectory = process.env.CHIMERA_E2E_ARTIFACT_DIR;
    if (artifactDirectory) {
      const screenshotPath = path.join(
        artifactDirectory,
        screenshotRunId,
        'profile-auto-update.e2e',
        'disables-scheduled-sync-while-keeping-manual-sync-available-in-both-uis',
        'main-options-open-before-cleanup.png',
      );
      try {
        fs.mkdirSync(path.dirname(screenshotPath), { recursive: true });
        await browser.saveScreenshot(screenshotPath);
        screenshotEvidence = ` Failure screenshot: ${screenshotPath}.`;
      } catch (screenshotError) {
        screenshotEvidence = ` Screenshot capture failed: ${String(screenshotError)}.`;
      }
    }
    throw new Error(
      `The main Profile update-options dialog did not open; last observed ${lastObserved}.${screenshotEvidence}`,
      { cause },
    );
  }
}

async function cancelMainOptions() {
  const cancelButton = await $('[data-slot="profile-update-options-cancel"]');
  await cancelButton.waitForExist({ timeout: 15_000 });
  await cancelButton.click();
  await browser.waitUntil(
    async () =>
      browser.execute(
        () =>
          document.querySelector(
            '[data-slot="modal-content"][data-state="open"]',
          ) === null,
      ),
    {
      timeout: 15_000,
      timeoutMsg:
        'The update-options dialog did not enter its closed state after cancelling.',
    },
  );
}

async function captureMainProfileState(uid: string) {
  let stateEvidence = 'UI state capture failed before inspection.';
  try {
    const state = await browser.execute((expectedUid) => {
      const details = Array.from(
        document.querySelectorAll<HTMLElement>('[data-slot="profile-detail"]'),
      );
      const detail = details.find(
        (candidate) => candidate.dataset.profileUid === expectedUid,
      );
      const trigger = detail?.querySelector<HTMLElement>(
        '[data-slot="profile-update-options-trigger"]',
      );

      return {
        url: location.href,
        details: details.map((candidate) => ({
          uid: candidate.dataset.profileUid ?? null,
          opacity: getComputedStyle(candidate).opacity,
          text: candidate.innerText.slice(0, 180),
        })),
        trigger: trigger
          ? {
              display: getComputedStyle(trigger).display,
              visibility: getComputedStyle(trigger).visibility,
              rects: trigger.getClientRects().length,
            }
          : null,
      };
    }, uid);
    stateEvidence = `Last Profile UI state: ${JSON.stringify(state)}.`;
  } catch (stateError) {
    stateEvidence = `Profile UI state capture failed: ${String(stateError)}.`;
  }
  let screenshotEvidence = '';
  const artifactDirectory = process.env.CHIMERA_E2E_ARTIFACT_DIR;
  if (artifactDirectory) {
    const screenshotPath = path.join(
      artifactDirectory,
      screenshotRunId,
      'profile-auto-update.e2e',
      'disables-scheduled-sync-while-keeping-manual-sync-available-in-both-uis',
      `main-profile-${uid}-before-cleanup-${++screenshotAttempt}.png`,
    );
    try {
      fs.mkdirSync(path.dirname(screenshotPath), { recursive: true });
      await browser.saveScreenshot(screenshotPath);
      screenshotEvidence = ` Failure screenshot: ${screenshotPath}.`;
    } catch (screenshotError) {
      screenshotEvidence = ` Screenshot capture failed: ${String(screenshotError)}.`;
    }
  }
  return `${stateEvidence}${screenshotEvidence}`;
}

async function openProfileDetail(uid: string) {
  const pathname = `/main/profiles/profile/detail/${uid}`;
  let currentPath = await browser.execute(() => location.pathname);
  if (currentPath !== '/main/profiles/profile') {
    await browser.execute((target) => {
      history.pushState({}, '', target);
      window.dispatchEvent(new PopStateEvent('popstate'));
    }, '/main/profiles/profile');
    await browser.waitUntil(
      async () => {
        currentPath = await browser.execute(() => location.pathname);
        return (
          currentPath === '/main/profiles/profile' &&
          (await $('[data-slot="profiles-list"]').isExisting())
        );
      },
      {
        timeout: 15_000,
        timeoutMsg: 'The main Profile list route did not render.',
      },
    );
  }

  const profileCard = await $(
    `[data-slot="profile-card"][data-profile-uid="${uid}"]`,
  );
  await profileCard.waitForDisplayed({ timeout: 15_000 });
  const detailLink = await profileCard.$('a');
  await detailLink.waitForDisplayed({ timeout: 15_000 });
  await detailLink.click();

  let lastObserved = currentPath;
  try {
    await browser.waitUntil(
      async () => {
        const state = await browser.execute((expectedUid) => {
          const details = Array.from(
            document.querySelectorAll<HTMLElement>(
              '[data-slot="profile-detail"]',
            ),
          );
          const detail = details.find(
            (candidate) => candidate.dataset.profileUid === expectedUid,
          );
          const trigger = detail?.querySelector<HTMLElement>(
            '[data-slot="profile-update-options-trigger"]',
          );
          const visible = (element: HTMLElement | null) => {
            if (!element || element.getClientRects().length === 0) return false;
            for (
              let current: HTMLElement | null = element;
              current;
              current = current.parentElement
            ) {
              const style = getComputedStyle(current);
              if (
                style.display === 'none' ||
                style.visibility === 'hidden' ||
                Number(style.opacity) < 0.99
              ) {
                return false;
              }
            }
            return true;
          };
          return {
            path: location.pathname,
            details: details.map(
              (candidate) => candidate.dataset.profileUid ?? null,
            ),
            hasVisibleOptionsTrigger: visible(trigger ?? null),
          };
        }, uid);
        lastObserved = `${state.path}; details=${state.details.join(',')}; visibleOptions=${state.hasVisibleOptionsTrigger}`;
        return state.path === pathname && state.hasVisibleOptionsTrigger;
      },
      {
        timeout: 30_000,
        timeoutMsg: `The subscription options for Profile ${uid} did not render.`,
      },
    );
  } catch (cause) {
    const failureState = await captureMainProfileState(uid);
    throw new Error(
      `The subscription options for Profile ${uid} did not render; last observed ${lastObserved}. ${failureState}`,
      { cause },
    );
  }
}

describe('remote profile auto-update', () => {
  it('disables scheduled sync while keeping manual sync available in both UIs', async () => {
    await withCleanup('remote profile auto-update E2E', async (defer) => {
      const originalProfiles = await readProfiles();
      const originalCurrent = originalProfiles.current ?? null;
      const verge = await invoke<VergeConfig>('get_verge_config');
      assert.equal(
        Boolean(verge.enable_system_proxy),
        false,
        'System proxy must be disabled before this loopback test.',
      );
      assert.equal(
        Boolean(verge.enable_tun_mode),
        false,
        'TUN must be disabled before this loopback test.',
      );

      let requestCount = 0;
      const server = createServer((_request, response) => {
        requestCount += 1;
        const body = 'proxies: []\n';
        response.writeHead(200, {
          'content-type': 'application/yaml',
          'content-length': Buffer.byteLength(body),
          'profile-title': fixtureProfileName,
        });
        response.end(body);
      });
      defer('close the local subscription server', () => closeServer(server));
      await new Promise<void>((resolve, reject) => {
        server.once('error', reject);
        server.listen(0, '127.0.0.1', resolve);
      });
      const address = server.address();
      assert.ok(address && typeof address !== 'string');

      const url = `http://127.0.0.1:${address.port}/subscription.yaml`;
      const outcome = await invoke<MutationOutcome<string>>('import_profile', {
        url,
        name: fixtureProfileName,
        option: {
          update_interval_minutes: 120,
          with_proxy: false,
          self_proxy: false,
        },
      });
      const uid = outcome.value;
      defer('restore the original Profile selection', async () => {
        const profiles = await readProfiles();
        if ((profiles.current ?? null) !== uid) return;
        committedValue(
          await invoke<MutationOutcome<null>>('activate_profile', {
            uid: originalCurrent,
          }),
          'Profile selection restoration',
        );
        await browser.waitUntil(
          async () =>
            ((await readProfiles()).current ?? null) === originalCurrent,
          {
            timeout: 30_000,
            timeoutMsg: 'The original Profile selection was not restored.',
          },
        );
      });
      defer('delete the imported test Profile', async () => {
        const profiles = await readProfiles();
        if (!profiles.items.some((item) => item.uid === uid)) return;
        assert.notEqual(
          profiles.current,
          uid,
          'The test Profile is still selected and cannot be deleted safely.',
        );
        committedValue(
          await invoke<MutationOutcome<null>>('delete_profile', { uid }),
          'Test Profile deletion',
        );
        await browser.waitUntil(
          async () =>
            !(await readProfiles()).items.some((item) => item.uid === uid),
          {
            timeout: 30_000,
            timeoutMsg: 'The imported test Profile was not removed.',
          },
        );
      });
      committedValue(outcome, 'Loopback subscription import');

      await browser.waitUntil(async () => requestCount >= 1, {
        timeout: 15_000,
        timeoutMsg: 'The import did not request the local subscription.',
      });
      assert.equal(remoteUpdateInterval(await readProfiles(), uid), 120);

      await browser.switchToWindow('main');
      await browser.setWindowSize(1240, 760);
      await ensureMainWindow();
      await browser.refresh();
      await browser.waitUntil(
        async () =>
          browser.execute(
            () =>
              (document.getElementById('root')?.childElementCount ?? 0) > 0 &&
              document.querySelector('[data-slot="app-root"]') !== null,
          ),
        {
          timeout: 30_000,
          timeoutMsg:
            'The main window did not reload the imported Profile fixture.',
        },
      );
      // Wait for the feature control itself. The shared route helper also waits
      // for every nested list animation, which is unrelated to this contract.
      await openProfileDetail(uid);
      await openMainUpdateOptions(uid);

      const optionsModal = await $(
        '[data-slot="modal-content"][data-state="open"]',
      );
      await optionsModal.waitForExist({ timeout: 15_000 });
      const autoUpdateSwitch = await $(
        '[data-slot="modal-content"][data-state="open"] [data-slot="profile-auto-update-switch"]',
      );
      await autoUpdateSwitch.waitForExist({ timeout: 15_000 });
      await waitForSwitchChecked(
        '[data-slot="modal-content"][data-state="open"] [data-slot="profile-auto-update-switch"]',
        true,
      );
      await autoUpdateSwitch.click();
      await waitForSwitchChecked(
        '[data-slot="modal-content"][data-state="open"] [data-slot="profile-auto-update-switch"]',
        false,
      );
      await saveMainOptions();
      await waitForInterval(uid, 0);
      await waitForPersistedInterval(uid, 0);

      const beforeManualSync = requestCount;
      const manualSync = await $(
        `[data-slot="profile-manual-sync"][data-profile-uid="${uid}"]`,
      );
      await manualSync.waitForExist({ timeout: 15_000 });
      await manualSync.click();
      try {
        await browser.waitUntil(async () => requestCount > beforeManualSync, {
          timeout: 30_000,
          timeoutMsg:
            'Manual sync did not fetch the local subscription while auto-update was off.',
        });
      } catch (cause) {
        throw new Error(
          `Manual sync did not fetch while auto-update was off; request count stayed ${requestCount}.`,
          { cause },
        );
      }

      await waitForSuccessfulSyncHistory(uid);

      await openMainUpdateOptions(uid);
      const cancelSwitch = await $(
        '[data-slot="modal-content"][data-state="open"] [data-slot="profile-auto-update-switch"]',
      );
      await cancelSwitch.waitForExist({ timeout: 15_000 });
      await waitForSwitchChecked(
        '[data-slot="modal-content"][data-state="open"] [data-slot="profile-auto-update-switch"]',
        false,
      );
      await cancelSwitch.click();
      await waitForSwitchChecked(
        '[data-slot="modal-content"][data-state="open"] [data-slot="profile-auto-update-switch"]',
        true,
      );
      await cancelMainOptions();
      await waitForInterval(uid, 0);
      await waitForPersistedInterval(uid, 0);

      await openMainUpdateOptions(uid);
      const reopenedSwitch = await $(
        '[data-slot="modal-content"][data-state="open"] [data-slot="profile-auto-update-switch"]',
      );
      await reopenedSwitch.waitForExist({ timeout: 15_000 });
      await waitForSwitchChecked(
        '[data-slot="modal-content"][data-state="open"] [data-slot="profile-auto-update-switch"]',
        false,
      );
      await reopenedSwitch.click();
      const intervalInput = await $(
        '[data-slot="modal-content"][data-state="open"] [data-slot="profile-update-interval"]',
      );
      await intervalInput.waitForExist({ timeout: 15_000 });
      assert.equal(await intervalInput.getValue(), '');
      await saveMainOptions();
      await waitForInterval(uid, 120);
      await waitForPersistedInterval(uid, 120);

      await invoke('create_legacy_window');
      await browser.waitUntil(
        async () => (await browser.getWindowHandles()).includes('legacy'),
        {
          timeout: 15_000,
          timeoutMsg: 'The legacy window was not created for the legacy flow.',
        },
      );
      await browser.switchToWindow('legacy');
      await browser.waitUntil(
        async () =>
          browser.execute(
            () =>
              document.querySelector(
                '[data-testid="sidebar-route-profiles"]',
              ) !== null,
          ),
        {
          timeout: 30_000,
          timeoutMsg: 'The legacy window did not mount its navigation sidebar.',
        },
      );
      const legacyUrl = new URL(await browser.getUrl());
      legacyUrl.pathname = '/profiles';
      legacyUrl.search = '';
      await browser.url(legacyUrl.href);
      const profileItem = await $(
        `[data-slot="profile-item"][data-profile-uid="${uid}"]`,
      );
      await profileItem.waitForExist({ timeout: 15_000 });
      const menuTrigger = await profileItem.$(
        '[data-slot="profile-menu-trigger"]',
      );
      await menuTrigger.waitForExist({ timeout: 15_000 });
      await menuTrigger.click();
      const editMenuItem = await $('[data-slot="profile-edit-menu-item"]');
      await editMenuItem.waitForExist({ timeout: 15_000 });
      await editMenuItem.click();

      const legacyInterval = await $(
        '[data-slot="legacy-profile-update-interval"] input',
      );
      await legacyInterval.waitForExist({ timeout: 15_000 });
      await legacyInterval.setValue('0');
      assert.equal(await legacyInterval.getValue(), '0');
      const legacySave = await $('button=OK');
      await legacySave.waitForExist({ timeout: 15_000 });
      await legacySave.click();
      await waitForInterval(uid, 0);
      await waitForPersistedInterval(uid, 0);

      const beforeLegacyManualSync = await invoke<ProfileSyncStatus>(
        'get_profile_sync_status',
        { uid },
      );
      const beforeLegacyRequestCount = requestCount;
      const legacyManualSync = await profileItem.$(
        '[data-slot="profile-update-button"]',
      );
      await legacyManualSync.waitForDisplayed({ timeout: 15_000 });
      await legacyManualSync.click();
      await browser.waitUntil(
        async () => requestCount > beforeLegacyRequestCount,
        {
          timeout: 30_000,
          timeoutMsg:
            'The legacy Profile update control did not fetch the local subscription.',
        },
      );
      await waitForExternalManualSync(
        uid,
        beforeLegacyManualSync.latest_run_sequence,
      );
    });
  });
});
