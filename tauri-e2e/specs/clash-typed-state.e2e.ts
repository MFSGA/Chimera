import assert from 'node:assert/strict';
import fs from 'node:fs';
import path from 'node:path';

// Contract: a direct typed Clash IPC patch changes the running core and the
// isolated clash-config.yaml; an invalid typed port patch leaves both intact.
// This is an integration check, not a claim about the settings page UI.

interface ClashInfo {
  secret?: string;
  server: string;
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

async function readRuntimeIPv6(): Promise<boolean> {
  const info = await invoke<ClashInfo>('get_clash_info');
  const response = await fetch(`http://${info.server}/configs`, {
    headers: info.secret
      ? { Authorization: `Bearer ${info.secret}` }
      : undefined,
  });
  if (!response.ok) {
    throw new Error(`Clash config query failed: ${response.status}`);
  }
  return ((await response.json()) as { ipv6: boolean }).ipv6;
}

async function waitForRuntimeIPv6(): Promise<boolean> {
  let value: boolean | undefined;
  await browser.waitUntil(
    async () => {
      try {
        value = await readRuntimeIPv6();
        return true;
      } catch {
        return false;
      }
    },
    {
      timeout: 30_000,
      timeoutMsg: 'The running Clash core did not expose /configs.',
    },
  );
  assert.notEqual(value, undefined);
  return value as boolean;
}

async function patchIPv6(value: boolean): Promise<void> {
  const config = await invoke<{ overrides: Record<string, unknown> }>(
    'get_clash_config',
  );
  await invoke<unknown>('patch_clash_config', {
    patch: { overrides: { ...config.overrides, ipv6: value } },
  });
}

function clashConfigPath(): string {
  const runtime = process.env.CHIMERA_E2E_RUNTIME_DIR;
  assert.ok(
    runtime,
    'CHIMERA_E2E_RUNTIME_DIR must be set by the WDIO harness.',
  );
  return path.join(runtime, 'config', 'clash-config.yaml');
}

function persistedIPv6(file: string): boolean | null {
  if (!fs.existsSync(file)) return null;
  const text = fs.readFileSync(file, 'utf8');
  const match = /^\s*ipv6:\s*(true|false)\s*$/m.exec(text);
  return match ? match[1] === 'true' : null;
}

async function waitForCommittedIPv6(expected: boolean) {
  const file = clashConfigPath();
  await browser.waitUntil(
    async () => {
      try {
        return (
          (await readRuntimeIPv6()) === expected &&
          persistedIPv6(file) === expected
        );
      } catch {
        return false;
      }
    },
    {
      timeout: 20_000,
      timeoutMsg: `runtime and clash-config.yaml did not commit ipv6=${String(expected)}.`,
    },
  );
}

describe('typed Clash saved-state ownership', () => {
  it('applies typed runtime overrides and rejects an invalid patch without changing state', async () => {
    await waitForApp();
    const original = await waitForRuntimeIPv6();
    const changed = !original;
    const file = clashConfigPath();
    const originalConfig = fs.existsSync(file)
      ? fs.readFileSync(file, 'utf8')
      : null;

    try {
      await assert.rejects(
        invoke<unknown>('patch_clash_config', {
          patch: { mixed_port: { start_port: 65_536 } },
        }),
      );
      assert.equal(await readRuntimeIPv6(), original);
      assert.equal(fs.existsSync(file), originalConfig !== null);
      if (originalConfig !== null) {
        assert.equal(fs.readFileSync(file, 'utf8'), originalConfig);
      }

      await patchIPv6(changed);
      await waitForCommittedIPv6(changed);

      await browser.refresh();
      await waitForApp();

      assert.equal(await readRuntimeIPv6(), changed);
      assert.equal(persistedIPv6(clashConfigPath()), changed);
    } finally {
      await patchIPv6(original);
      await waitForCommittedIPv6(original);
    }
  });
});
