import assert from 'node:assert/strict';
import { readClashRuntimeConfig } from '../clash-runtime.js';

const settingsPath = '/main/settings/clash';
const ipv6SwitchSelector =
  '[data-slot="ipv6-switch-container"] [role="switch"]';

interface ClashRuntimeState {
  ipv6: boolean;
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

async function readClashRuntimeState(): Promise<ClashRuntimeState> {
  const config = await readClashRuntimeConfig<{ ipv6: boolean }>();
  return { ipv6: config.ipv6 };
}

async function openClashSettings() {
  const currentHref = await browser.getUrl();
  await browser.url(new URL(settingsPath, currentHref).href);
  await waitForApp();

  const toggle = await $(ipv6SwitchSelector);
  await toggle.waitForExist({ timeout: 15_000 });
}

async function readIPv6Switch(): Promise<boolean | null> {
  return browser.execute((selector) => {
    const toggle = document.querySelector<HTMLElement>(selector);
    const state = toggle?.getAttribute('data-state');
    if (state === 'checked') return true;
    if (state === 'unchecked') return false;
    return null;
  }, ipv6SwitchSelector);
}

async function setIPv6(enabled: boolean) {
  const runtime = await readClashRuntimeState();
  const ui = await readIPv6Switch();

  if (runtime.ipv6 === enabled && ui === enabled) return;

  if (runtime.ipv6 === enabled && ui !== enabled) {
    await browser.refresh();
    await waitForApp();
  } else {
    const clicked = await browser.execute((selector) => {
      const toggle = document.querySelector<HTMLButtonElement>(selector);
      if (!toggle || toggle.disabled) return false;
      toggle.click();
      return true;
    }, ipv6SwitchSelector);
    assert.equal(clicked, true, 'The IPv6 runtime switch was not clickable.');
  }

  await browser.waitUntil(
    async () => {
      const current = await readClashRuntimeState();
      return current.ipv6 === enabled && (await readIPv6Switch()) === enabled;
    },
    {
      timeout: 15_000,
      timeoutMsg: `IPv6 runtime and UI state did not become ${String(enabled)}.`,
    },
  );
}

describe('Chimera IPv6 runtime setting', () => {
  it('persists through the typed runtime transaction and restores the original value', async () => {
    await waitForApp();
    await openClashSettings();

    const original = (await readClashRuntimeState()).ipv6;
    const changed = !original;

    try {
      await setIPv6(changed);
      await browser.refresh();
      await waitForApp();

      await browser.waitUntil(
        async () => {
          const current = await readClashRuntimeState();
          return (
            current.ipv6 === changed && (await readIPv6Switch()) === changed
          );
        },
        {
          timeout: 15_000,
          timeoutMsg: 'IPv6 did not remain persisted after a page reload.',
        },
      );
    } finally {
      await setIPv6(original);
      await browser.refresh();
      await waitForApp();
      assert.equal((await readClashRuntimeState()).ipv6, original);
      assert.equal(await readIPv6Switch(), original);
    }
  });
});
