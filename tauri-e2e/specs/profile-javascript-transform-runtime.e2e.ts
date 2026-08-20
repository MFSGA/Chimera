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
type LogSpan = 'log' | 'info' | 'warn' | 'error';

interface RuntimeTransformDiagnostics {
  revision: number;
  output: {
    scopes: Record<string, Record<string, Array<[LogSpan, string]>>>;
    global: Record<string, Array<[LogSpan, string]>>;
  };
}

interface ProfileResponse {
  type: 'remote' | 'local' | 'merge' | 'script';
  uid: string;
  name: string;
  chain?: string[];
}

interface ProfilesResponse {
  current: string | null;
  items: ProfileResponse[];
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

async function setScopedChain(
  uid: string,
  transforms: string[],
): Promise<void> {
  requireApplied(
    await invoke<MutationOutcome<null>>('set_profile_transform_chain', {
      uid,
      transforms,
    }),
    'scoped transform chain update',
  );
}

describe('Chimera JavaScript transform runtime lifecycle', () => {
  it('executes a JavaScript transform in a scoped runtime chain', async () => {
    const suffix = Date.now();
    const initialProfiles = await readProfiles();
    const previousCurrent = initialProfiles.current;
    let localUid: string | null = null;
    let javascriptUid: string | null = null;

    await waitForCoreRunning();

    try {
      localUid = requireApplied(
        await invoke<MutationOutcome<string>>('create_profile', {
          item: {
            type: 'local',
            uid: null,
            name: `javascript-source-${suffix}`,
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
        }),
        'local profile creation',
      );

      javascriptUid = requireApplied(
        await invoke<MutationOutcome<string>>('create_profile', {
          item: {
            type: 'script',
            name: `javascript-transform-${suffix}`,
            desc: null,
            script_type: 'javascript',
          },
          fileData: [
            'export default function (config) {',
            '  config["unified-delay"] = true;',
            '  console.info("e2e javascript transform executed");',
            '  return config;',
            '}',
            '',
          ].join('\n'),
        }),
        'JavaScript profile creation',
      );

      requireApplied(
        await invoke<MutationOutcome<null>>('activate_profile', {
          uid: localUid,
        }),
        'source profile activation',
      );
      await waitForCoreRunning();
      await waitForUnifiedDelay(false);

      await setScopedChain(localUid, [javascriptUid]);
      await waitForCoreRunning();
      await waitForUnifiedDelay(true);

      const profiles = await readProfiles();
      const source = profiles.items.find((item) => item.uid === localUid);
      assert.deepEqual(source?.chain, [javascriptUid]);

      const diagnostics = await invoke<RuntimeTransformDiagnostics | null>(
        'get_runtime_transform_diagnostics',
      );
      assert.ok(diagnostics);
      assert.ok(diagnostics.revision > 0);
      assert.deepEqual(diagnostics.output.scopes[localUid]?.[javascriptUid], [
        ['info', 'e2e javascript transform executed'],
      ]);

      await setScopedChain(localUid, []);
      await waitForCoreRunning();
      await waitForUnifiedDelay(false);

      const detachedDiagnostics =
        await invoke<RuntimeTransformDiagnostics | null>(
          'get_runtime_transform_diagnostics',
        );
      assert.ok(detachedDiagnostics);
      assert.ok(detachedDiagnostics.revision > diagnostics.revision);
      assert.deepEqual(detachedDiagnostics.output.scopes[localUid], {});
    } finally {
      if (localUid) {
        await setScopedChain(localUid, []).catch(() => undefined);
      }
      if (javascriptUid) {
        await invoke<MutationOutcome<null>>('delete_profile', {
          uid: javascriptUid,
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
