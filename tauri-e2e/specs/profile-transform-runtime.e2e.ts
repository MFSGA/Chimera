import assert from 'node:assert/strict';
import fs from 'node:fs';
import path from 'node:path';
import type {
  MutationOutcome,
  ProfileDocument_Deserialize,
} from '../../frontend/interface/src/ipc/bindings.js';
import {
  appliedValue,
  committedValue,
  createProfile,
  findProfileUid,
  invoke,
  localConfigProfileRequest,
  overlayProfileRequest,
  readProfiles,
  scopedTransformsOf,
  setGlobalTransforms,
  setScopedTransforms,
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

function runtimeProductPath(): string {
  const runtimeRoot = process.env.CHIMERA_E2E_RUNTIME_DIR;
  assert.ok(runtimeRoot, 'CHIMERA_E2E_RUNTIME_DIR is not configured.');
  return path.join(runtimeRoot, 'config', 'runtime', 'clash-config.yaml');
}

async function waitForUnifiedDelay(expected: boolean): Promise<void> {
  const product = runtimeProductPath();
  let lastValue = 'missing';
  await browser.waitUntil(
    async () => {
      if (!fs.existsSync(product)) {
        lastValue = 'runtime product missing';
        return false;
      }
      const contents = fs.readFileSync(product, 'utf8');
      const match = contents.match(/^unified-delay:\s*(true|false)\s*$/m);
      lastValue = match?.[1] ?? 'field missing';
      return lastValue === String(expected);
    },
    {
      timeout: 30_000,
      timeoutMsg: `Runtime unified-delay did not become ${expected}. Last value: ${lastValue}`,
    },
  );
}

async function createLocalProfile(
  name: string,
): Promise<MutationOutcome<string>> {
  return createProfile(
    localConfigProfileRequest(name),
    [
      'unified-delay: false',
      'proxies: []',
      'proxy-groups: []',
      'rules: []',
      '',
    ].join('\n'),
  );
}

async function createOverlayProfile(
  name: string,
): Promise<MutationOutcome<string>> {
  return createProfile(overlayProfileRequest(name), 'unified-delay: true\n');
}

describe('Chimera transform profile runtime lifecycle', () => {
  // Contract: with a real core running, this integration test creates a local
  // config and overlay through current Profile IPC, activates the config, and
  // changes scoped then global transform IDs. The Profile document and
  // generated runtime YAML are independent result sources. Old `{ item }`
  // requests or chain commands fail setup; cleanup restores selection and the
  // original global transforms before deleting only this run's UIDs.
  it('applies and removes merge transforms through scoped and global chains', async () => {
    const suffix = Date.now();
    const localName = `transform-source-${suffix}`;
    const mergeName = `transform-merge-${suffix}`;
    await withCleanup('transform Profile runtime lifecycle', async (defer) => {
      const initialProfiles = await readProfiles();
      let selectionMayHaveChanged = false;
      let globalTransformsMayHaveChanged = false;

      defer('restore the original Profile selection', async () => {
        if (!selectionMayHaveChanged) return;
        committedValue(
          await invoke<MutationOutcome<null>>('activate_profile', {
            uid: initialProfiles.current ?? null,
          }),
          'Profile selection restoration',
        );
      });
      defer('restore original global transforms', async () => {
        if (!globalTransformsMayHaveChanged) return;
        committedValue(
          await setGlobalTransforms(initialProfiles.global_transforms ?? []),
          'global transform restoration',
        );
      });
      defer('clear transforms on the test source', async () => {
        const uid = await findProfileUid(localName);
        if (!uid) return;
        committedValue(
          await setScopedTransforms(uid, []),
          'scoped transform cleanup',
        );
      });
      defer('delete the test overlay', async () => {
        const uid = await findProfileUid(mergeName);
        if (!uid) return;
        committedValue(
          await invoke<MutationOutcome<null>>('delete_profile', {
            uid,
          }),
          'overlay deletion',
        );
      });
      defer('delete the test source', async () => {
        const uid = await findProfileUid(localName);
        if (!uid) return;
        committedValue(
          await invoke<MutationOutcome<null>>('delete_profile', {
            uid,
          }),
          'source Profile deletion',
        );
      });

      await waitForCoreRunning();
      const local = await createLocalProfile(localName);
      const localUid = local.value;
      committedValue(local, 'local Profile creation');
      const overlay = await createOverlayProfile(mergeName);
      const mergeUid = overlay.value;
      committedValue(overlay, 'overlay Profile creation');

      selectionMayHaveChanged = true;
      appliedValue(
        await invoke<MutationOutcome<null>>('activate_profile', {
          uid: localUid,
        }),
        'source Profile activation',
      );
      await waitForCoreRunning();
      await waitForUnifiedDelay(false);

      appliedValue(
        await setScopedTransforms(localUid, [mergeUid]),
        'scoped overlay update',
      );
      await waitForCoreRunning();
      await waitForUnifiedDelay(true);

      let profiles: ProfileDocument_Deserialize = await readProfiles();
      const source = profiles.items.find((item) => item.uid === localUid);
      assert.deepEqual(source && scopedTransformsOf(source), [mergeUid]);

      appliedValue(
        await setScopedTransforms(localUid, []),
        'scoped overlay removal',
      );
      await waitForCoreRunning();
      await waitForUnifiedDelay(false);

      globalTransformsMayHaveChanged = true;
      appliedValue(
        await setGlobalTransforms([mergeUid]),
        'global overlay update',
      );
      await waitForCoreRunning();
      await waitForUnifiedDelay(true);
      profiles = await readProfiles();
      assert.deepEqual(profiles.global_transforms, [mergeUid]);

      appliedValue(await setGlobalTransforms([]), 'global overlay removal');
      await waitForCoreRunning();
      await waitForUnifiedDelay(false);
    });
  });
});
