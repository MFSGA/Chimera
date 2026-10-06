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

function readRuntimeDnsEnabled(product: string): string {
  let contents: string;
  try {
    contents = fs.readFileSync(product, 'utf8');
  } catch (error) {
    if ((error as NodeJS.ErrnoException).code === 'ENOENT') {
      return 'runtime product missing';
    }
    throw error;
  }

  const dns = contents.match(
    /^dns:[ \t]*\r?\n((?:[ \t]+[^\r\n]*(?:\r?\n|$))*)/m,
  )?.[1];
  return (
    dns?.match(/^[ \t]+enable:[ \t]*(true|false)[ \t]*$/m)?.[1] ??
    (dns ? 'dns.enable missing' : 'dns section missing')
  );
}

async function waitForDnsEnabled(expected: boolean): Promise<void> {
  const product = runtimeProductPath();
  try {
    await browser.waitUntil(
      async () => readRuntimeDnsEnabled(product) === String(expected),
      {
        timeout: 30_000,
        timeoutMsg: `Timed out waiting for runtime dns.enable=${expected}.`,
      },
    );
  } catch (error) {
    throw new Error(
      `Runtime dns.enable did not become ${expected}. Final observation: ${readRuntimeDnsEnabled(product)}.`,
      { cause: error },
    );
  }
}

describe('Chimera Lua transform runtime lifecycle', () => {
  // Contract: a real running core consumes a locally stored Lua transform
  // attached through the current config definition. The generated runtime YAML
  // and transform diagnostics must both show its effect; the persisted Profile
  // item must carry the same transform UID. IPC fixture setup is an integration
  // boundary, not a UI workflow. dns.enable proves transform output because the
  // default core guard supplies unified-delay=true after profile transforms.
  // Cleanup restores the original selection and removes only this test's data.
  it('executes a Lua transform in a scoped runtime chain', async () => {
    const suffix = Date.now();
    const localName = `lua-source-${suffix}`;
    const luaName = `lua-transform-${suffix}`;
    await withCleanup('Lua transform Profile lifecycle', async (defer) => {
      const initialProfiles: ProfileDocument_Deserialize = await readProfiles();
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
        const uid = await findProfileUid(luaName);
        if (!uid) return;
        committedValue(
          await invoke<MutationOutcome<null>>('delete_profile', { uid }),
          'Lua Profile deletion',
        );
      });
      defer('delete the test source', async () => {
        const uid = await findProfileUid(localName);
        if (!uid) return;
        committedValue(
          await invoke<MutationOutcome<null>>('delete_profile', { uid }),
          'source Profile deletion',
        );
      });

      await waitForCoreRunning();
      const local = await createProfile(
        localConfigProfileRequest(localName),
        [
          'dns:',
          '  enable: false',
          'proxies: []',
          'proxy-groups: []',
          'rules: []',
          '',
        ].join('\n'),
      );
      const localUid = local.value;
      committedValue(local, 'local Profile creation');
      const script = await createProfile(
        scriptProfileRequest(luaName, 'lua'),
        [
          'config["dns"]["enable"] = true',
          'info("e2e lua transform executed")',
          'return config',
          '',
        ].join('\n'),
      );
      const luaUid = script.value;
      committedValue(script, 'Lua Profile creation');

      selectionMayHaveChanged = true;
      appliedValue(
        await invoke<MutationOutcome<null>>('activate_profile', {
          uid: localUid,
        }),
        'source Profile activation',
      );
      await waitForCoreRunning();
      await waitForDnsEnabled(false);

      appliedValue(
        await setScopedTransforms(localUid, [luaUid]),
        'scoped Lua transform update',
      );
      await waitForCoreRunning();
      await waitForDnsEnabled(true);

      const profiles = await readProfiles();
      const source = profiles.items.find((item) => item.uid === localUid);
      assert.deepEqual(source && scopedTransformsOf(source), [luaUid]);

      const diagnostics = await invoke<RuntimeTransformDiagnostics | null>(
        'get_runtime_transform_diagnostics',
      );
      assert.ok(diagnostics);
      assert.ok(diagnostics.revision > 0);
      assert.deepEqual(diagnostics.output.scopes[localUid]?.[luaUid], [
        ['info', 'e2e lua transform executed'],
      ]);

      appliedValue(
        await setScopedTransforms(localUid, []),
        'scoped Lua transform removal',
      );
      await waitForCoreRunning();
      await waitForDnsEnabled(false);

      const detachedDiagnostics =
        await invoke<RuntimeTransformDiagnostics | null>(
          'get_runtime_transform_diagnostics',
        );
      assert.ok(detachedDiagnostics);
      assert.ok(detachedDiagnostics.revision > diagnostics.revision);
      assert.deepEqual(detachedDiagnostics.output.scopes[localUid] ?? {}, {});
    });
  });
});
