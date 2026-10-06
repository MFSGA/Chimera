import assert from 'node:assert/strict';
import type {
  MutationOutcome,
  ProfileDocument_Deserialize,
} from '../../frontend/interface/src/ipc/bindings.js';
import {
  committedValue,
  invoke,
  readProfiles,
  withCleanup,
} from './profile-fixtures.js';

type CoreState = 'Running' | { Stopped: string | null };

async function waitForCoreRunning(): Promise<void> {
  let lastState = 'unknown';
  await browser.waitUntil(
    async () => {
      const [state] =
        await invoke<[CoreState, number, string]>('get_core_status');
      lastState = JSON.stringify(state);
      return state === 'Running';
    },
    {
      timeout: 30_000,
      timeoutMsg: `The core did not become running. Last state: ${lastState}`,
    },
  );
}

async function createLocalProfile(name: string): Promise<string> {
  const beforeIds = new Set(
    (await readProfiles()).items.map((item) => item.uid),
  );
  const currentUrl = new URL(await browser.getUrl());
  currentUrl.pathname = '/main/profiles/local';
  currentUrl.search = '';
  await browser.url(currentUrl.href);
  currentUrl.pathname = '/main/profiles/profile';
  currentUrl.search = '?action=ImportLocalProfile';
  await browser.url(currentUrl.href);

  const nameInput = await $('input[name="name"]');
  await nameInput.waitForDisplayed({ timeout: 15_000 });
  await nameInput.setValue(name);

  const okButton = await $('button=OK');
  await okButton.waitForClickable({ timeout: 15_000 });
  await okButton.click();

  let uid: string | null = null;
  await browser.waitUntil(
    async () => {
      const profiles = await readProfiles();
      uid =
        profiles.items.find(
          (item) => item.name === name && !beforeIds.has(item.uid),
        )?.uid ?? null;
      return Boolean(uid);
    },
    {
      timeout: 45_000,
      timeoutMsg: `The local profile ${name} was not created.`,
    },
  );
  assert.ok(uid);
  await waitForCoreRunning();
  return uid;
}

describe('Chimera profile reorder persistence', () => {
  it('persists the final reordered profile list without requiring a runtime rebuild', async () => {
    // Contract: create two uniquely named local Profiles through the UI,
    // reorder them through the shared API, and verify the persisted order after
    // refresh. The runtime is shared across specs, so cleanup restores the
    // original selection and UID order and deletes only these exact fixtures.
    const prefix = `profile-reorder-e2e-${Date.now()}-`;
    const firstName = `${prefix}a`;
    const secondName = `${prefix}b`;
    const original = await readProfiles();
    assert.equal(
      original.items.some((item) =>
        [firstName, secondName].includes(item.name),
      ),
      false,
      'The isolated runtime already contains a profile owned by this test.',
    );

    await withCleanup('Profile reorder persistence', async (defer) => {
      defer('restore the original profile state', async () => {
        let profiles: ProfileDocument_Deserialize = await readProfiles();
        if (profiles.current !== original.current) {
          committedValue(
            await invoke<MutationOutcome<null>>('activate_profile', {
              uid: original.current,
            }),
            'Original profile selection restoration',
          );
          await browser.waitUntil(
            async () => (await readProfiles()).current === original.current,
            {
              timeout: 30_000,
              timeoutMsg: 'The original profile selection was not restored.',
            },
          );
        }

        profiles = await readProfiles();
        const testProfiles = profiles.items.filter((item) =>
          [firstName, secondName].includes(item.name),
        );
        for (const item of testProfiles) {
          committedValue(
            await invoke<MutationOutcome<null>>('delete_profile', {
              uid: item.uid,
            }),
            `Test profile ${item.name} deletion`,
          );
          await browser.waitUntil(
            async () =>
              !(await readProfiles()).items.some(
                (profile) => profile.uid === item.uid,
              ),
            {
              timeout: 30_000,
              timeoutMsg: `Test profile ${item.name} remained after deletion.`,
            },
          );
        }

        profiles = await readProfiles();
        const originalOrder = original.items.map((item) => item.uid);
        const originalUidSet = new Set(originalOrder);
        assert.deepEqual(
          new Set(profiles.items.map((item) => item.uid)),
          originalUidSet,
          'Profile cleanup changed the original profile set.',
        );
        if (
          profiles.items.map((item) => item.uid).join('|') !==
          originalOrder.join('|')
        ) {
          committedValue(
            await invoke<MutationOutcome<null>>('reorder_profiles_by_list', {
              list: originalOrder,
            }),
            'Original profile order restoration',
          );
          await browser.waitUntil(
            async () =>
              (await readProfiles()).items.map((item) => item.uid).join('|') ===
              originalOrder.join('|'),
            {
              timeout: 30_000,
              timeoutMsg: 'The original profile order was not restored.',
            },
          );
        }
        await waitForCoreRunning();
      });

      const firstUid = await createLocalProfile(firstName);
      await browser.waitUntil(
        async () => (await readProfiles()).current === firstUid,
        {
          timeout: 45_000,
          timeoutMsg: 'The first local profile was not activated.',
        },
      );
      const secondUid = await createLocalProfile(secondName);

      const before = await readProfiles();
      const originalCurrentOrder = before.items.map((item) => item.uid);
      const reordered = originalCurrentOrder.map((uid) => {
        if (uid === firstUid) return secondUid;
        if (uid === secondUid) return firstUid;
        return uid;
      });
      assert.notDeepEqual(reordered, originalCurrentOrder);

      for (const [list, operation] of [
        [reordered, 'Initial profile reorder'],
        [originalCurrentOrder, 'Profile order reset'],
        [reordered, 'Final profile reorder'],
      ] as const) {
        committedValue(
          await invoke<MutationOutcome<null>>('reorder_profiles_by_list', {
            list,
          }),
          operation,
        );
      }

      await browser.waitUntil(
        async () => {
          const profiles = await readProfiles();
          return (
            profiles.items.map((item) => item.uid).join('|') ===
            reordered.join('|')
          );
        },
        {
          timeout: 30_000,
          timeoutMsg: 'The final profile order was not committed.',
        },
      );

      await browser.refresh();
      await browser.waitUntil(
        async () => {
          const profiles = await readProfiles();
          return (
            profiles.items.map((item) => item.uid).join('|') ===
            reordered.join('|')
          );
        },
        {
          timeout: 30_000,
          timeoutMsg:
            'The final profile order was not persisted after refresh.',
        },
      );
    });
  });
});
