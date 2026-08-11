import assert from 'node:assert/strict';
import fs from 'node:fs';
import path from 'node:path';

interface ProfileItem {
  uid: string;
  name: string;
}

interface ProfilesResponse {
  current: string | null;
  items: ProfileItem[];
}

type CoreState = 'Running' | { Stopped: string | null };

async function invoke<T>(command: string, args?: Record<string, unknown>) {
  return browser.execute(
    async (name, payload) => {
      const tauri = (
        window as typeof window & {
          __TAURI_INTERNALS__: {
            invoke: (
              command: string,
              args?: Record<string, unknown>,
            ) => Promise<T>;
          };
        }
      ).__TAURI_INTERNALS__;
      return tauri.invoke(name, payload);
    },
    command,
    args,
  );
}

async function readProfiles(): Promise<ProfilesResponse> {
  return invoke<ProfilesResponse>('get_profiles');
}

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

function runtimePaths() {
  const runtimeRoot = process.env.CHIMERA_E2E_RUNTIME_DIR;
  assert.ok(runtimeRoot, 'CHIMERA_E2E_RUNTIME_DIR is not configured.');
  const runtimeDirectory = path.join(runtimeRoot, 'config', 'runtime');
  return {
    product: path.join(runtimeDirectory, 'clash-config.yaml'),
    candidates: path.join(runtimeDirectory, '.candidates'),
  };
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

async function waitForProductReplacement(
  previousMtimeMs: number,
): Promise<void> {
  const { product, candidates } = runtimePaths();
  await browser.waitUntil(
    async () => {
      if (!fs.existsSync(product)) return false;
      const candidateFiles = fs.existsSync(candidates)
        ? fs
            .readdirSync(candidates)
            .filter((name) => name.startsWith('candidate-'))
        : [];
      return (
        fs.statSync(product).mtimeMs > previousMtimeMs &&
        candidateFiles.length === 0
      );
    },
    {
      timeout: 30_000,
      timeoutMsg:
        'Profile reorder did not finish promoting and cleaning its runtime candidate.',
    },
  );
}

async function removeProfilesWithPrefix(prefix: string): Promise<void> {
  const profiles = await readProfiles();
  for (const item of profiles.items.filter((profile) =>
    profile.name.startsWith(prefix),
  )) {
    await invoke('delete_profile', { uid: item.uid });
  }
  await waitForCoreRunning();
}

describe('Chimera coalesced profile reorder rebuild', () => {
  it('persists reordered profiles and rebuilds the runtime product in the background', async () => {
    const prefix = 'coalesced-reorder-e2e-';
    await removeProfilesWithPrefix(prefix);
    const original = await readProfiles();
    const created: string[] = [];
    const suffix = Date.now();

    try {
      created.push(await createLocalProfile(`${prefix}a-${suffix}`));
      await browser.waitUntil(
        async () => (await readProfiles()).current === created[0],
        {
          timeout: 45_000,
          timeoutMsg: 'The first local profile was not activated.',
        },
      );
      const { product: initialProduct } = runtimePaths();
      await browser.waitUntil(async () => fs.existsSync(initialProduct), {
        timeout: 30_000,
        timeoutMsg: 'The initial runtime product was not promoted.',
      });
      created.push(await createLocalProfile(`${prefix}b-${suffix}`));

      const before = await readProfiles();
      const originalCurrentOrder = before.items.map((item) => item.uid);
      const first = created[0];
      const second = created[1];
      const reordered = originalCurrentOrder.map((uid) => {
        if (uid === first) return second;
        if (uid === second) return first;
        return uid;
      });
      assert.notDeepEqual(reordered, originalCurrentOrder);

      const { product } = runtimePaths();
      assert.equal(fs.existsSync(product), true);
      const productMtimeMs = fs.statSync(product).mtimeMs;

      await invoke('reorder_profiles_by_list', { list: reordered });
      await invoke('reorder_profiles_by_list', { list: originalCurrentOrder });
      await invoke('reorder_profiles_by_list', { list: reordered });

      await waitForProductReplacement(productMtimeMs);
      await waitForCoreRunning();

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
    } finally {
      for (const uid of created.reverse()) {
        await invoke('delete_profile', { uid }).catch(() => undefined);
      }
      await removeProfilesWithPrefix(prefix).catch(() => undefined);
      const remaining = await readProfiles().catch(() => null);
      if (remaining) {
        const originalRemainingOrder = original.items
          .map((item) => item.uid)
          .filter((uid) => remaining.items.some((item) => item.uid === uid));
        if (originalRemainingOrder.length === remaining.items.length) {
          await invoke('reorder_profiles_by_list', {
            list: originalRemainingOrder,
          }).catch(() => undefined);
        }
      }
      await waitForCoreRunning();
    }
  });
});
