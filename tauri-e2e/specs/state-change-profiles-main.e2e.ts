import assert from 'node:assert/strict';
import { openMainRoute } from './main-window.js';
import {
  committedValue,
  createProfile,
  deleteProfile,
  findProfileUid,
  overlayProfileRequest,
  readProfiles,
  withCleanup,
} from './profile-fixtures.js';

const mergeProfilesPath = '/main/profiles/merge';
const name = `State changed merge fixture ${process.pid}-${Date.now()}`;

async function waitForLoadedList(): Promise<void> {
  await browser.waitUntil(
    async () =>
      browser.execute(() => {
        const content = document.querySelector(
          '[data-slot="profiles-content"]',
        );
        return Boolean(
          content?.querySelector(
            '[data-slot="profiles-list"], [data-slot="profiles-empty-list"]',
          ),
        );
      }),
    {
      timeout: 20_000,
      timeoutMsg: 'The merge Profiles list never completed its first render.',
    },
  );
}

async function cardFor(uid: string): Promise<boolean> {
  return browser.execute((id) => {
    const cards = document.querySelectorAll(
      '[data-slot="profile-card"][data-profile-uid]',
    );
    return [...cards].some(
      (card) => card.getAttribute('data-profile-uid') === id,
    );
  }, uid);
}

async function waitForCard(uid: string, expected: boolean): Promise<void> {
  let lastSeen: boolean | null = null;
  try {
    await browser.waitUntil(
      async () => {
        lastSeen = await cardFor(uid);
        return lastSeen === expected;
      },
      {
        timeout: 15_000,
        interval: 250,
        timeoutMsg: 'Profile card did not synchronize.',
      },
    );
  } catch (cause) {
    throw new Error(
      `Profile ${uid}: expected UI card ${expected}, last observed ${lastSeen}.`,
      { cause },
    );
  }
}

// Contract (T01/T02/T06/T07):
// - A loaded merge Profile page reflects a different caller's committed
//   profile creation and deletion without navigation or reloading.
// - Backend get_profiles confirms the UID separately from the rendered card.
// - The test only owns a synthetic overlay Profile and deletes that exact UID.
// - Uses the real IPC for fixture mutation, so this proves external
//   commit -> event -> main UI, not the create-dialog user operation.
// - A missing or stale Profiles event makes the mounted list assertion fail.
describe('StateChangedEvent: main Profile list synchronization', () => {
  it('adds and removes a Profile card after an external commit', async () => {
    await withCleanup('main Profiles notification', async (defer) => {
      await openMainRoute(mergeProfilesPath);
      await waitForLoadedList();
      assert.equal(await findProfileUid(name), undefined);

      defer('delete the owned overlay fixture', async () => {
        const uid = await findProfileUid(name);
        if (!uid) return;
        const profiles = await readProfiles();
        assert.notEqual(
          profiles.current,
          uid,
          'Refusing to delete an active fixture without restoring selection.',
        );
        committedValue(
          await deleteProfile(uid),
          'Delete owned overlay fixture',
        );
      });

      const uid = committedValue(
        await createProfile(overlayProfileRequest(name), 'proxies: []\n'),
        'Create overlay fixture',
      );
      assert.equal(await findProfileUid(name), uid);
      await waitForCard(uid, true);

      committedValue(await deleteProfile(uid), 'Delete overlay fixture');
      assert.equal(await findProfileUid(name), undefined);
      await waitForCard(uid, false);
    });
  });
});
