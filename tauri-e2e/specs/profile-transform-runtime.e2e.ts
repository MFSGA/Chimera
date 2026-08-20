import assert from 'node:assert/strict';
import fs from 'node:fs';
import path from 'node:path';

type MutationOutcome<T> =
  | { status: 'applied'; value: T }
  | {
      status: 'committed_degraded';
      value: T;
      degradations: Array<{ message: string }>;
    };

type CoreState = 'Running' | { Stopped: string | null };

interface ProfileResponse {
  type: 'remote' | 'local' | 'merge' | 'script';
  uid: string;
  name: string;
  chain?: string[];
}

interface ProfilesResponse {
  current: string | null;
  items: ProfileResponse[];
  global_transforms: string[];
}

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

function requireApplied<T>(outcome: MutationOutcome<T>, operation: string): T {
  assert.equal(
    outcome.status,
    'applied',
    `${operation} degraded: ${
      outcome.status === 'committed_degraded'
        ? outcome.degradations.map((item) => item.message).join('; ')
        : 'unknown outcome'
    }`,
  );
  return outcome.value;
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

async function createLocalProfile(name: string): Promise<string> {
  const outcome = await invoke<MutationOutcome<string>>('create_profile', {
    item: {
      type: 'local',
      uid: null,
      name,
      file: null,
      desc: null,
      updated: null,
      symlinks: null,
      chain: [],
    },
    fileData: [
      'unified-delay: false',
      'proxies: []',
      'proxy-groups: []',
      'rules: []',
      '',
    ].join('\n'),
  });
  return requireApplied(outcome, 'local profile creation');
}

async function createMergeProfile(name: string): Promise<string> {
  const outcome = await invoke<MutationOutcome<string>>('create_profile', {
    item: { type: 'merge', name, desc: null },
    fileData: 'unified-delay: true\n',
  });
  return requireApplied(outcome, 'merge profile creation');
}

async function setScopedChain(
  uid: string,
  transforms: string[],
): Promise<void> {
  const outcome = await invoke<MutationOutcome<null>>(
    'set_profile_transform_chain',
    { uid, transforms },
  );
  requireApplied(outcome, 'scoped transform chain update');
}

async function setGlobalChain(transforms: string[]): Promise<void> {
  const outcome = await invoke<MutationOutcome<null>>(
    'set_global_transform_chain',
    { transforms },
  );
  requireApplied(outcome, 'global transform chain update');
}

describe('Chimera transform profile runtime lifecycle', () => {
  it('applies and removes merge transforms through scoped and global chains', async () => {
    const suffix = Date.now();
    const localName = `transform-source-${suffix}`;
    const mergeName = `transform-merge-${suffix}`;
    const initialProfiles = await readProfiles();
    const previousCurrent = initialProfiles.current;
    let localUid: string | null = null;
    let mergeUid: string | null = null;

    await waitForCoreRunning();

    try {
      localUid = await createLocalProfile(localName);
      mergeUid = await createMergeProfile(mergeName);

      requireApplied(
        await invoke<MutationOutcome<null>>('activate_profile', {
          uid: localUid,
        }),
        'source profile activation',
      );
      await waitForCoreRunning();
      await waitForUnifiedDelay(false);

      await setScopedChain(localUid, [mergeUid]);
      await waitForCoreRunning();
      await waitForUnifiedDelay(true);

      let profiles = await readProfiles();
      const source = profiles.items.find((item) => item.uid === localUid);
      assert.deepEqual(source?.chain, [mergeUid]);

      await setScopedChain(localUid, []);
      await waitForCoreRunning();
      await waitForUnifiedDelay(false);

      await setGlobalChain([mergeUid]);
      await waitForCoreRunning();
      await waitForUnifiedDelay(true);
      profiles = await readProfiles();
      assert.deepEqual(profiles.global_transforms, [mergeUid]);

      await setGlobalChain([]);
      await waitForCoreRunning();
      await waitForUnifiedDelay(false);
    } finally {
      await setGlobalChain([]).catch(() => undefined);
      if (localUid) {
        await setScopedChain(localUid, []).catch(() => undefined);
      }
      if (mergeUid) {
        await invoke<MutationOutcome<null>>('delete_profile', {
          uid: mergeUid,
        }).catch(() => undefined);
      }
      if (localUid) {
        await invoke<MutationOutcome<null>>('delete_profile', {
          uid: localUid,
        }).catch(() => undefined);
      }
      if (previousCurrent) {
        await invoke<MutationOutcome<null>>('activate_profile', {
          uid: previousCurrent,
        }).catch(() => undefined);
      }
    }
  });
});
