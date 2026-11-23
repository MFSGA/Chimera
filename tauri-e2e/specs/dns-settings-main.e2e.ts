import assert from 'node:assert/strict';
import { ensureMainWindow, openMainRoute } from './main-window.js';

/**
 * Contract DNS-SETTINGS-MAIN: desktop UI → typed Clash settings IPC. Start
 * with a supported core and profile-owned DNS/Hosts; edit one nameserver
 * through visible inputs and controls, verify the saved value and control
 * source, then restore test-owned settings. The YAML case checks shape
 * rejection and draft-only apply through visible controls. IPC is setup,
 * authority, and cleanup only. It does not test live resolver behavior, cold
 * start, or DNS network success.
 */
const route = '/main/settings/clash/dns';

type DnsValue = {
  nameserver?: string[];
  [key: string]: unknown;
};

type ClashSettings = {
  dns_control?: 'profile' | 'chimera';
  hosts_control?: 'profile' | 'chimera';
  dns?: DnsValue;
  hosts?: Record<string, string | string[]>;
};

type DnsFeatures = {
  keys: { kind: 'ignored' } | { kind: 'values'; value: string[] };
};

async function invoke<T>(
  command: string,
  args?: Record<string, unknown>,
): Promise<T> {
  return browser.execute(
    async (name, payload) => {
      const bridge = (
        window as typeof window & {
          __TAURI_INTERNALS__: {
            invoke: (
              name: string,
              args?: Record<string, unknown>,
            ) => Promise<T>;
          };
        }
      ).__TAURI_INTERNALS__;
      return bridge.invoke(name, payload);
    },
    command,
    args,
  );
}

async function openDnsPage() {
  await openMainRoute(route);
  await browser.waitUntil(
    async () =>
      browser.execute(
        () =>
          location.pathname.replace(/\/$/, '') === '/main/settings/clash/dns' &&
          document.querySelector('[data-slot="dns-settings-page"]') !== null,
      ),
    { timeout: 20_000, timeoutMsg: 'DNS settings page did not render.' },
  );
}

async function firstNameserverInput() {
  await browser.waitUntil(
    async () => {
      const card = await $(
        '[data-slot="dns-upstream-settings"] [data-dns-key="nameserver"]',
      );
      return card.isExisting();
    },
    { timeout: 15_000, timeoutMsg: 'DNS upstream inputs did not render.' },
  );
  return $(
    '[data-slot="dns-upstream-settings"] [data-dns-key="nameserver"] input',
  );
}

describe('typed DNS settings in the new UI', () => {
  let originalWindowSize: { width: number; height: number } | undefined;
  let originalLanguage: string | null | undefined;

  before(async () => {
    await ensureMainWindow();
    originalWindowSize = await browser.getWindowSize();
    originalLanguage = await browser.execute(() =>
      localStorage.getItem(btoa('paraglide-language-cache')),
    );
    await browser.setWindowSize(1240, 700);
    await browser.execute(() => {
      localStorage.setItem(btoa('paraglide-language-cache'), 'en');
    });
  });

  after(async () => {
    const cleanupErrors: Error[] = [];
    const attemptCleanup = async (
      label: string,
      cleanup: () => Promise<void>,
    ) => {
      try {
        await cleanup();
      } catch (error) {
        cleanupErrors.push(new Error(`Failed to ${label}.`, { cause: error }));
      }
    };

    await attemptCleanup('focus the main window', ensureMainWindow);
    const language = originalLanguage;
    if (language !== undefined) {
      await attemptCleanup('restore the original language', async () => {
        await browser.execute((value) => {
          const key = btoa('paraglide-language-cache');
          if (value === null) localStorage.removeItem(key);
          else localStorage.setItem(key, value);
        }, language);
      });
    }
    const windowSize = originalWindowSize;
    if (windowSize) {
      await attemptCleanup('restore the original window size', async () => {
        await browser.setWindowSize(windowSize.width, windowSize.height);
      });
    }
    if (cleanupErrors.length) {
      throw new AggregateError(cleanupErrors, 'DNS settings cleanup failed.');
    }
  });

  it('shows the HTTP/3 option only when the selected core supports it', async () => {
    await openDnsPage();
    const features = await invoke<DnsFeatures>('get_dns_features');

    if (features.keys.kind === 'ignored') {
      const control = await $('[data-slot="dns-control-settings"]');
      assert.equal(await control.isExisting(), false);
      return;
    }

    await $('[data-slot="dns-control-settings"]').waitForDisplayed({
      timeout: 15_000,
    });
    const h3 = await $$(
      '[data-slot="dns-general-settings"] [aria-label="Prefer HTTP/3"]',
    );
    assert.equal(
      (await h3.length) > 0,
      features.keys.value.includes('prefer-h3'),
      'The UI must follow the runtime feature matrix.',
    );
  });

  it('shows profile draft notices from each independent control source', async () => {
    await openDnsPage();
    const saved = await invoke<ClashSettings>('get_clash_config');
    assert.ok(saved.dns);
    assert.ok(saved.hosts);

    const dnsNotice = await $('[data-slot="dns-profile-control-notice"]');
    const hostsNotice = await $(
      '[data-slot="dns-hosts-profile-control-notice"]',
    );
    assert.equal(
      await dnsNotice.isExisting(),
      (saved.dns_control ?? 'profile') !== 'chimera',
    );
    assert.equal(
      await hostsNotice.isExisting(),
      (saved.hosts_control ?? 'profile') !== 'chimera',
    );
  });

  it('takes over DNS and Hosts through the visible controls', async () => {
    const original = await invoke<ClashSettings>('get_clash_config');
    assert.ok(original.dns);
    assert.ok(original.hosts);
    assert.equal(original.dns_control ?? 'profile', 'profile');
    assert.equal(original.hosts_control ?? 'profile', 'profile');

    let operationError: unknown;
    let operationFailed = false;
    try {
      await openDnsPage();
      const dnsSwitch = await $('[data-slot="dns-control-dns-switch"]');
      const hostsSwitch = await $('[data-slot="dns-control-hosts-switch"]');
      await dnsSwitch.waitForClickable({ timeout: 15_000 });
      await dnsSwitch.click();
      await browser.waitUntil(
        async () =>
          (await invoke<ClashSettings>('get_clash_config')).dns_control ===
          'chimera',
        { timeout: 20_000, timeoutMsg: 'DNS takeover was not persisted.' },
      );
      await browser.waitUntil(
        async () => (await dnsSwitch.getAttribute('aria-checked')) === 'true',
        { timeout: 15_000, timeoutMsg: 'DNS takeover UI did not update.' },
      );
      await browser.waitUntil(
        async () =>
          !(await $('[data-slot="dns-profile-control-notice"]').isExisting()),
        {
          timeout: 15_000,
          timeoutMsg: 'The DNS profile notice did not disappear.',
        },
      );

      await hostsSwitch.waitForClickable({ timeout: 15_000 });
      await hostsSwitch.click();
      await browser.waitUntil(
        async () =>
          (await invoke<ClashSettings>('get_clash_config')).hosts_control ===
          'chimera',
        { timeout: 20_000, timeoutMsg: 'Hosts takeover was not persisted.' },
      );
      await browser.waitUntil(
        async () => (await hostsSwitch.getAttribute('aria-checked')) === 'true',
        { timeout: 15_000, timeoutMsg: 'Hosts takeover UI did not update.' },
      );
      await browser.waitUntil(
        async () =>
          !(await $(
            '[data-slot="dns-hosts-profile-control-notice"]',
          ).isExisting()),
        {
          timeout: 15_000,
          timeoutMsg: 'The Hosts profile notice did not disappear.',
        },
      );
    } catch (error) {
      operationError = error;
      operationFailed = true;
    }

    const cleanupErrors: unknown[] = [];
    try {
      await invoke('patch_clash_config', {
        patch: {
          dns_control: original.dns_control ?? 'profile',
          hosts_control: original.hosts_control ?? 'profile',
          dns: original.dns,
          hosts: original.hosts ?? {},
        },
      });
      await browser.waitUntil(
        async () => {
          const restored = await invoke<ClashSettings>('get_clash_config');
          return (
            (restored.dns_control ?? 'profile') ===
              (original.dns_control ?? 'profile') &&
            (restored.hosts_control ?? 'profile') ===
              (original.hosts_control ?? 'profile') &&
            JSON.stringify(restored.dns) === JSON.stringify(original.dns) &&
            JSON.stringify(restored.hosts ?? {}) ===
              JSON.stringify(original.hosts ?? {})
          );
        },
        { timeout: 20_000, timeoutMsg: 'DNS control cleanup did not finish.' },
      );
    } catch (error) {
      cleanupErrors.push(error);
    }

    const errors = [
      ...(operationFailed ? [operationError] : []),
      ...cleanupErrors,
    ];
    if (errors.length === 1) throw errors[0];
    if (errors.length > 1) {
      throw new AggregateError(errors, 'DNS takeover and cleanup failed.');
    }
  });

  it('saves a profile-owned DNS draft and retains it after page refresh', async () => {
    const original = await invoke<ClashSettings>('get_clash_config');
    const features = await invoke<DnsFeatures>('get_dns_features');
    assert.equal(
      features.keys.kind,
      'values',
      'The isolated E2E app must use a core that supports DNS editing.',
    );
    assert.equal(original.dns_control ?? 'profile', 'profile');
    assert.equal(original.hosts_control ?? 'profile', 'profile');
    assert.ok(original.dns, 'Saved DNS settings are required for this test.');
    assert.ok(
      original.hosts,
      'Saved Hosts settings are required for this test.',
    );

    const previous = original.dns?.nameserver?.[0];
    const next =
      previous === 'https://dns.google/dns-query'
        ? 'https://one.one.one.one/dns-query'
        : 'https://dns.google/dns-query';

    let operationError: unknown;
    let operationFailed = false;
    try {
      await openDnsPage();
      const input = await firstNameserverInput();
      await input.waitForDisplayed({ timeout: 15_000 });
      await input.waitForEnabled({ timeout: 15_000 });
      await input.setValue(next);

      const save = await $('[data-slot="dns-save"]');
      await save.waitForEnabled({ timeout: 15_000 });
      await save.click();

      await browser.waitUntil(
        async () =>
          (await invoke<ClashSettings>('get_clash_config')).dns
            ?.nameserver?.[0] === next,
        { timeout: 20_000, timeoutMsg: 'The DNS draft was not persisted.' },
      );
      assert.equal(
        await input.getValue(),
        next,
        'The saved value must remain visible before refreshing.',
      );

      await browser.refresh();
      await openDnsPage();
      const restoredInput = await firstNameserverInput();
      assert.equal(await restoredInput.getValue(), next);
    } catch (error) {
      operationError = error;
      operationFailed = true;
    }

    const cleanupErrors: unknown[] = [];
    try {
      await invoke('patch_clash_config', {
        patch: {
          dns_control: original.dns_control ?? 'profile',
          hosts_control: original.hosts_control ?? 'profile',
          dns: original.dns,
          hosts: original.hosts ?? {},
        },
      });
    } catch (error) {
      cleanupErrors.push(error);
    }
    try {
      await browser.waitUntil(
        async () => {
          const restored = await invoke<ClashSettings>('get_clash_config');
          return (
            JSON.stringify(restored.dns) === JSON.stringify(original.dns) &&
            JSON.stringify(restored.hosts) ===
              JSON.stringify(original.hosts ?? {}) &&
            (restored.dns_control ?? 'profile') ===
              (original.dns_control ?? 'profile') &&
            (restored.hosts_control ?? 'profile') ===
              (original.hosts_control ?? 'profile')
          );
        },
        {
          timeout: 20_000,
          timeoutMsg: 'DNS cleanup did not restore settings.',
        },
      );
    } catch (error) {
      cleanupErrors.push(error);
    }

    const errors = [
      ...(operationFailed ? [operationError] : []),
      ...cleanupErrors,
    ];
    if (errors.length === 1) throw errors[0];
    if (errors.length > 1) {
      throw new AggregateError(errors, 'DNS save and/or cleanup failed.');
    }
  });

  it('validates YAML and applies valid input to the draft only', async () => {
    const original = await invoke<ClashSettings>('get_clash_config');
    const features = await invoke<DnsFeatures>('get_dns_features');
    assert.equal(features.keys.kind, 'values');
    assert.ok(original.dns);
    assert.ok(original.hosts);

    const previous = original.dns.nameserver?.[0];
    const next =
      previous === 'https://dns.google/dns-query'
        ? 'https://one.one.one.one/dns-query'
        : 'https://dns.google/dns-query';

    let operationError: unknown;
    let operationFailed = false;
    try {
      await openDnsPage();
      await $('[data-slot="dns-yaml-editor-trigger"]').click();
      const editor = await $('[data-slot="dns-yaml-editor-input"]');
      await editor.waitForDisplayed({ timeout: 10_000 });
      await editor.setValue(
        `dns:\n  nameserver: ${JSON.stringify(next)}\nhosts: ${JSON.stringify(original.hosts)}`,
      );
      await $('[data-slot="dns-yaml-editor-apply"]').click();
      await $('[data-slot="dns-yaml-editor-error"]').waitForDisplayed({
        timeout: 10_000,
      });
      assert.equal(
        (await invoke<ClashSettings>('get_clash_config')).dns?.nameserver?.[0],
        previous,
        'Invalid YAML must not persist a partial update.',
      );

      await editor.setValue(
        `dns:\n  enable: null\nhosts: ${JSON.stringify(original.hosts)}`,
      );
      await $('[data-slot="dns-yaml-editor-apply"]').click();
      await $('[data-slot="dns-yaml-editor-error"]').waitForDisplayed({
        timeout: 10_000,
      });
      assert.equal(
        (await invoke<ClashSettings>('get_clash_config')).dns?.enable,
        original.dns?.enable,
        'Null cannot replace a non-nullable typed DNS field.',
      );

      await editor.setValue(
        `dns:\n  nameserver:\n    - ${JSON.stringify(next)}\nhosts: ${JSON.stringify(original.hosts)}`,
      );
      await $('[data-slot="dns-yaml-editor-apply"]').click();
      const input = await firstNameserverInput();
      assert.equal(await input.getValue(), next);
      assert.equal(
        (await invoke<ClashSettings>('get_clash_config')).dns?.nameserver?.[0],
        previous,
        'Applying YAML changes only the draft until Save is clicked.',
      );
    } catch (error) {
      operationError = error;
      operationFailed = true;
    }

    const cleanupErrors: unknown[] = [];
    try {
      const editor = await $('[data-slot="dns-yaml-editor-input"]');
      if (await editor.isDisplayed()) await browser.keys('Escape');
      const discard = await $('[data-slot="dns-discard"]');
      if ((await discard.isExisting()) && (await discard.isEnabled())) {
        await discard.click();
      }
      await invoke('patch_clash_config', {
        patch: {
          dns_control: original.dns_control ?? 'profile',
          hosts_control: original.hosts_control ?? 'profile',
          dns: original.dns,
          hosts: original.hosts ?? {},
        },
      });
      await browser.waitUntil(
        async () => {
          const restored = await invoke<ClashSettings>('get_clash_config');
          return (
            JSON.stringify(restored.dns) === JSON.stringify(original.dns) &&
            JSON.stringify(restored.hosts ?? {}) ===
              JSON.stringify(original.hosts ?? {}) &&
            (restored.dns_control ?? 'profile') ===
              (original.dns_control ?? 'profile') &&
            (restored.hosts_control ?? 'profile') ===
              (original.hosts_control ?? 'profile')
          );
        },
        {
          timeout: 15_000,
          timeoutMsg: 'YAML cleanup did not restore settings.',
        },
      );
    } catch (error) {
      cleanupErrors.push(error);
    }

    const errors = [
      ...(operationFailed ? [operationError] : []),
      ...cleanupErrors,
    ];
    if (errors.length === 1) throw errors[0];
    if (errors.length > 1) {
      throw new AggregateError(
        errors,
        'DNS YAML operation and cleanup failed.',
      );
    }
  });
});
