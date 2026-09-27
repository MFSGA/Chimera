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
  readProfiles,
  scopedTransformsOf,
  scriptProfileRequest,
  setScopedTransforms,
  withCleanup,
} from './profile-fixtures.js';

type CoreState = 'Running' | { Stopped: string | null };
type LogSpan = 'log' | 'info' | 'warn' | 'error';

interface RuntimeTransformDiagnostics {
  revision: number;
  output: {
    scopes: Record<string, Record<string, Array<[LogSpan, string]>>>;
    global: Record<string, Array<[LogSpan, string]>>;
  };
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

describe('Chimera JavaScript transform runtime lifecycle', () => {
  // Contract: a real running core consumes a locally stored JS transform
  // attached through the current config definition. The generated runtime YAML
  // and transform diagnostics must both show its effect; the persisted Profile
  // item must carry the same transform UID. IPC fixture setup is an integration
  // boundary, not a UI workflow. Cleanup restores the original selection and
  // detaches/deletes only this test's profiles while preserving failures.
  it('executes a JavaScript transform in a scoped runtime chain', async () => {
    const suffix = Date.now();
    const localName = `javascript-source-${suffix}`;
    const javascriptName = `javascript-transform-${suffix}`;
    await withCleanup(
      'JavaScript transform Profile lifecycle',
      async (defer) => {
        const initialProfiles: ProfileDocument_Deserialize =
          await readProfiles();
        let selectionMayHaveChanged = false;

        defer('restore the original Profile selection', async () => {
          if (!selectionMayHaveChanged) return;
          committedValue(
            await invoke<MutationOutcome<null>>('activate_profile', {
              uid: initialProfiles.current ?? null,
            }),
            'Profile selection restoration',
          );
        });
        defer('detach the test transform', async () => {
          const uid = await findProfileUid(localName);
          if (!uid) return;
          committedValue(
            await setScopedTransforms(uid, []),
            'scoped transform cleanup',
          );
        });
        defer('delete the test script', async () => {
          const uid = await findProfileUid(javascriptName);
          if (!uid) return;
          committedValue(
            await invoke<MutationOutcome<null>>('delete_profile', {
              uid,
            }),
            'JavaScript Profile deletion',
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
        const local = await createProfile(
          localConfigProfileRequest(localName),
          [
            'unified-delay: false',
            'proxies: []',
            'proxy-groups: []',
            'rules: []',
            '',
          ].join('\n'),
        );
        const localUid = local.value;
        committedValue(local, 'local Profile creation');
        const script = await createProfile(
          scriptProfileRequest(javascriptName, 'javascript'),
          [
            'export default function (config) {',
            '  config["unified-delay"] = true;',
            '  console.info("e2e javascript transform executed");',
            '  return config;',
            '}',
            '',
          ].join('\n'),
        );
        const javascriptUid = script.value;
        committedValue(script, 'JavaScript Profile creation');

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
          await setScopedTransforms(localUid, [javascriptUid]),
          'scoped JavaScript transform update',
        );
        await waitForCoreRunning();
        await waitForUnifiedDelay(true);

        const profiles = await readProfiles();
        const source = profiles.items.find((item) => item.uid === localUid);
        assert.deepEqual(source && scopedTransformsOf(source), [javascriptUid]);

        const diagnostics = await invoke<RuntimeTransformDiagnostics | null>(
          'get_runtime_transform_diagnostics',
        );
        assert.ok(diagnostics);
        assert.ok(diagnostics.revision > 0);
        assert.deepEqual(diagnostics.output.scopes[localUid]?.[javascriptUid], [
          ['info', 'e2e javascript transform executed'],
        ]);

        appliedValue(
          await setScopedTransforms(localUid, []),
          'scoped JavaScript transform removal',
        );
        await waitForCoreRunning();
        await waitForUnifiedDelay(false);

        const detachedDiagnostics =
          await invoke<RuntimeTransformDiagnostics | null>(
            'get_runtime_transform_diagnostics',
          );
        assert.ok(detachedDiagnostics);
        assert.ok(detachedDiagnostics.revision > diagnostics.revision);
        assert.deepEqual(detachedDiagnostics.output.scopes[localUid], {});
      },
    );
  });
});
