import assert from 'node:assert/strict';

const storageKey = 'custom-css-compiled';
const styleElementId = 'chimera-custom-css';
const cssVariable = '--chimera-storage-sync-probe';

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

describe('Chimera storage event resynchronization', () => {
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
      const cacheKey = `nyanpasu-kv-:${btoa(key)}`;
      const raw = localStorage.getItem(cacheKey);
      return raw === null ? null : JSON.parse(raw);
    }, storageKey);
    assert.equal(cachedValue, css);

    await invokeStorage('remove_storage_item');
    await browser.execute(
      (styleId) => document.getElementById(styleId)?.remove(),
      styleElementId,
    );
  });
});
