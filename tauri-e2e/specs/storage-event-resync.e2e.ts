import assert from 'node:assert/strict';

/*
Contract: in the isolated legacy window, a backend write must reach the
mounted custom-CSS provider without a reload. The rendered CSS, local cache,
and a revisioned backend read must agree. If event propagation fails, the
bounded UI wait fails instead of passing from the IPC snapshot alone.
Boundary: Tauri IPC prepares and observes the value; WebView event handling
and CSS application are exercised in the real desktop app.
*/
const storageKey = 'custom-css-compiled';
const styleElementId = 'chimera-custom-css';
const cssVariable = '--chimera-storage-sync-probe';
const cacheKey = `nyanpasu-kv-:${btoa(storageKey)}`;

let originalValue: string | null = null;
let originalCache: string | null = null;
let stateCaptured = false;

async function invokeStorage(
  command: 'set_storage_item' | 'remove_storage_item',
  value?: string,
) {
  await browser.execute(
    async (commandName, key, serializedValue) => {
      const internals = (
        window as Window & {
          __TAURI_INTERNALS__?: {
            invoke: (
              command: string,
              args: Record<string, unknown>,
            ) => Promise<unknown>;
          };
        }
      ).__TAURI_INTERNALS__;

      if (!internals) {
        throw new Error('Tauri page internals are unavailable.');
      }

      const args: Record<string, unknown> = { key };
      if (serializedValue !== undefined) {
        args.value = serializedValue;
      }

      await internals.invoke(commandName, args);
    },
    command,
    storageKey,
    value,
  );
}

async function readStorageSnapshot() {
  return browser.execute(async (key) => {
    const internals = (
      window as Window & {
        __TAURI_INTERNALS__?: {
          invoke: <T>(
            command: string,
            args: Record<string, unknown>,
          ) => Promise<T>;
        };
      }
    ).__TAURI_INTERNALS__;

    if (!internals) {
      throw new Error('Tauri page internals are unavailable.');
    }

    return internals.invoke<{ value: string | null; revision: number }>(
      'get_storage_item',
      { key },
    );
  }, storageKey);
}

async function attemptCleanup(
  errors: Error[],
  label: string,
  operation: () => Promise<void>,
) {
  try {
    await operation();
  } catch (cause) {
    errors.push(new Error(`Failed to restore ${label}.`, { cause }));
  }
}

describe('Chimera storage event resynchronization', () => {
  beforeEach(async () => {
    const snapshot = await readStorageSnapshot();
    originalValue = snapshot.value;
    originalCache = await browser.execute(
      (key) => localStorage.getItem(key),
      cacheKey,
    );
    stateCaptured = true;
  });

  afterEach(async () => {
    if (!stateCaptured) return;

    let originalCss: string | null = null;
    if (originalValue !== null) {
      try {
        const parsed: unknown = JSON.parse(originalValue);
        if (typeof parsed === 'string') originalCss = parsed;
      } catch {
        // Invalid prior data is restored as-is, while its style element is cleared.
      }
    }

    const cleanupErrors: Error[] = [];
    await attemptCleanup(cleanupErrors, 'backend storage', async () => {
      if (originalValue === null) {
        await invokeStorage('remove_storage_item');
      } else {
        await invokeStorage('set_storage_item', originalValue);
      }
    });
    await attemptCleanup(
      cleanupErrors,
      'backend storage snapshot',
      async () => {
        assert.equal((await readStorageSnapshot()).value, originalValue);
      },
    );
    await attemptCleanup(cleanupErrors, 'local storage cache', async () => {
      const restoredCache = await browser.execute(
        (key, value) => {
          if (value === null) localStorage.removeItem(key);
          else localStorage.setItem(key, value);
          return localStorage.getItem(key);
        },
        cacheKey,
        originalCache,
      );
      assert.equal(restoredCache, originalCache);
    });
    await attemptCleanup(cleanupErrors, 'custom CSS style', async () => {
      const restoredCss = await browser.execute(
        (styleId, css) => {
          let style = document.getElementById(
            styleId,
          ) as HTMLStyleElement | null;
          if (css === null) {
            style?.remove();
            return null;
          }
          if (!style) {
            style = document.createElement('style');
            style.id = styleId;
            document.head.appendChild(style);
          }
          style.textContent = css;
          return style.textContent;
        },
        styleElementId,
        originalCss,
      );
      assert.equal(restoredCss, originalCss);
    });
    stateCaptured = false;

    if (cleanupErrors.length > 0) {
      throw new AggregateError(
        cleanupErrors,
        'Storage event test cleanup failed.',
      );
    }
  });

  it('applies an external backend storage update to the mounted UI', async () => {
    await browser.waitUntil(
      async () =>
        browser.execute(
          () => (document.getElementById('root')?.childElementCount ?? 0) > 0,
        ),
      { timeout: 15_000, timeoutMsg: 'The application root did not render.' },
    );

    await invokeStorage('remove_storage_item');
    await browser.execute(
      (styleId) => document.getElementById(styleId)?.remove(),
      styleElementId,
    );

    const css = `:root { ${cssVariable}: 7px; }`;
    await invokeStorage('set_storage_item', JSON.stringify(css));

    await browser.waitUntil(
      async () =>
        browser.execute(
          (styleId, variableName) => {
            const style = document.getElementById(styleId);
            const appliedValue = getComputedStyle(document.documentElement)
              .getPropertyValue(variableName)
              .trim();
            return (
              style?.textContent?.includes(variableName) &&
              appliedValue === '7px'
            );
          },
          styleElementId,
          cssVariable,
        ),
      {
        timeout: 15_000,
        timeoutMsg: 'The backend storage event did not update the mounted UI.',
      },
    );

    const cachedValue = await browser.execute((key) => {
      const keyForCache = `nyanpasu-kv-:${btoa(key)}`;
      const raw = localStorage.getItem(keyForCache);
      return raw === null ? null : JSON.parse(raw);
    }, storageKey);
    assert.equal(cachedValue, css);

    const snapshot = await readStorageSnapshot();
    assert.equal(snapshot.value, JSON.stringify(css));
    assert.equal(Number.isSafeInteger(snapshot.revision), true);
    assert.ok(snapshot.revision > 0);
  });
});
