import assert from 'node:assert/strict';
import fs from 'node:fs';
import path from 'node:path';

interface VergeConfig {
  enable_auto_check_update?: boolean;
}

async function waitForApp() {
  await browser.waitUntil(
    async () =>
      browser.execute(
        () => (document.getElementById('root')?.childElementCount ?? 0) > 0,
      ),
    { timeout: 30_000, timeoutMsg: 'The Chimera frontend did not render.' },
  );
}

async function invoke<T>(
  command: string,
  args?: Record<string, unknown>,
): Promise<T> {
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

async function readAutoUpdate(): Promise<boolean> {
  const config = await invoke<VergeConfig>('get_verge_config');
  return config.enable_auto_check_update ?? true;
}

async function patchAutoUpdate(value: boolean): Promise<void> {
  await invoke<null>('patch_verge_config', {
    payload: { enable_auto_check_update: value },
  });
}

function applicationConfigPath(): string {
  const runtime = process.env.CHIMERA_E2E_RUNTIME_DIR;
  assert.ok(
    runtime,
    'CHIMERA_E2E_RUNTIME_DIR must be set by the WDIO harness.',
  );
  return path.join(runtime, 'config', 'application.yaml');
}

function persistedAutoUpdate(file: string): boolean | null {
  if (!fs.existsSync(file)) return null;
  const text = fs.readFileSync(file, 'utf8');
  const match = /^enable_auto_check_update:\s*(true|false)\s*$/m.exec(text);
  if (!match) return null;
  return match[1] === 'true';
}

async function waitForTypedPersistence(expected: boolean) {
  const file = applicationConfigPath();
  await browser.waitUntil(() => persistedAutoUpdate(file) === expected, {
    timeout: 15_000,
    timeoutMsg: `application.yaml did not persist enable_auto_check_update=${String(expected)}.`,
  });
}

describe('typed application state ownership', () => {
  it('persists legacy IPC patches through application.yaml and mirrors them back', async () => {
    await waitForApp();

    const original = await readAutoUpdate();
    const changed = !original;

    try {
      await patchAutoUpdate(changed);

      await browser.waitUntil(
        async () => (await readAutoUpdate()) === changed,
        {
          timeout: 15_000,
          timeoutMsg:
            'Legacy get_verge_config did not observe the typed application patch.',
        },
      );
      await waitForTypedPersistence(changed);

      await browser.refresh();
      await waitForApp();

      assert.equal(await readAutoUpdate(), changed);
      assert.equal(persistedAutoUpdate(applicationConfigPath()), changed);
    } finally {
      await patchAutoUpdate(original);
      await browser.waitUntil(
        async () => (await readAutoUpdate()) === original,
        {
          timeout: 15_000,
          timeoutMsg:
            'Application setting did not restore through the typed state owner.',
        },
      );
      await waitForTypedPersistence(original);
    }
  });
});
