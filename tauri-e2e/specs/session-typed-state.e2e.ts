import assert from 'node:assert/strict';
import fs from 'node:fs';
import path from 'node:path';

interface WindowState {
  width: number;
  height: number;
  x: number;
  y: number;
  maximized: boolean;
  fullscreen: boolean;
}

interface VergeConfig {
  window_type?: 'legacy' | 'main';
  window_size_state?: WindowState;
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

function sessionStatePath(): string {
  const runtime = process.env.CHIMERA_E2E_RUNTIME_DIR;
  assert.ok(runtime, 'CHIMERA_E2E_RUNTIME_DIR must be set by the WDIO harness.');
  return path.join(runtime, 'config', 'session-state.yaml');
}

function persistedMainWindowState(file: string): WindowState | null {
  if (!fs.existsSync(file)) return null;

  const lines = fs.readFileSync(file, 'utf8').split(/\r?\n/);
  const mainIndex = lines.findIndex((line) => /^\s+main:\s*$/.test(line));
  if (mainIndex < 0) return null;

  const mainIndent = lines[mainIndex].match(/^\s*/)?.[0].length ?? 0;
  const values = new Map<string, string>();
  for (let index = mainIndex + 1; index < lines.length; index += 1) {
    const line = lines[index];
    if (!line.trim()) continue;
    const indent = line.match(/^\s*/)?.[0].length ?? 0;
    if (indent <= mainIndent) break;
    const match = /^\s+(width|height|x|y|maximized|fullscreen):\s*(.+)\s*$/.exec(
      line,
    );
    if (match) values.set(match[1], match[2]);
  }

  const width = Number(values.get('width'));
  const height = Number(values.get('height'));
  const x = Number(values.get('x'));
  const y = Number(values.get('y'));
  const maximized = values.get('maximized');
  const fullscreen = values.get('fullscreen');
  if (
    !Number.isFinite(width) ||
    !Number.isFinite(height) ||
    !Number.isFinite(x) ||
    !Number.isFinite(y) ||
    !maximized ||
    !fullscreen
  ) {
    return null;
  }

  return {
    width,
    height,
    x,
    y,
    maximized: maximized === 'true',
    fullscreen: fullscreen === 'true',
  };
}

async function saveCurrentWindowState(label: string): Promise<void> {
  await invoke<null>('save_window_size_state', { label });
}

async function waitForMirroredState(): Promise<WindowState> {
  const file = sessionStatePath();
  let persisted: WindowState | null = null;
  await browser.waitUntil(
    async () => {
      persisted = persistedMainWindowState(file);
      const legacy = await invoke<VergeConfig>('get_verge_config');
      return Boolean(
        persisted &&
          legacy.window_size_state &&
          persisted.width > 0 &&
          persisted.height > 0 &&
          persisted.width === legacy.window_size_state.width &&
          persisted.height === legacy.window_size_state.height &&
          persisted.x === legacy.window_size_state.x &&
          persisted.y === legacy.window_size_state.y &&
          persisted.maximized === legacy.window_size_state.maximized &&
          persisted.fullscreen === legacy.window_size_state.fullscreen,
      );
    },
    {
      timeout: 15_000,
      timeoutMsg:
        'session-state.yaml and the legacy window compatibility mirror did not converge.',
    },
  );
  assert.ok(persisted);
  return persisted;
}

describe('typed session/window state ownership', () => {
  it('persists a real window capture through session-state.yaml and mirrors it to legacy config', async () => {
    await waitForApp();
    const config = await invoke<VergeConfig>('get_verge_config');
    const label = config.window_type === 'main' ? 'main' : 'legacy';
    const original = await browser.getWindowSize();
    const changed = {
      width: Math.max(840, original.width + 48),
      height: Math.max(680, original.height + 36),
    };

    try {
      await browser.setWindowSize(changed.width, changed.height);
      await saveCurrentWindowState(label);
      const persisted = await waitForMirroredState();

      assert.ok(persisted.width > 0);
      assert.ok(persisted.height > 0);
      assert.ok(fs.readFileSync(sessionStatePath(), 'utf8').includes('window_state:'));

      await browser.refresh();
      await waitForApp();
      const afterRefresh = await invoke<VergeConfig>('get_verge_config');
      assert.equal(afterRefresh.window_size_state?.width, persisted.width);
      assert.equal(afterRefresh.window_size_state?.height, persisted.height);
    } finally {
      await browser.setWindowSize(original.width, original.height);
      await saveCurrentWindowState(label);
      await waitForMirroredState();
    }
  });
});
