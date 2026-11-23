import assert from 'node:assert/strict';
import fs from 'node:fs';
import path from 'node:path';
import { readAppMessageCatalog } from './app-message-catalog.js';
import { openMainRoute } from './main-window.js';
import {
  appliedValue,
  committedValue,
  createProfile,
  invoke,
  localConfigProfileRequest,
  readProfiles,
  runCleanupActions,
} from './profile-fixtures.js';

const profileName = `TDD Provider Details ${Date.now()}`;
const proxyProviderName = 'e2e-proxy-provider';
const ruleProviderName = 'e2e-rule-provider';
const proxyNodeName = 'E2E Provider SOCKS';
const providerDir = path.join(
  process.env.CHIMERA_E2E_RUNTIME_DIR ?? '',
  'data',
  'providers-detail-fixture',
);

const renderFixture = (
  proxyFile: string,
  ruleFile: string,
) => `mixed-port: 27895
allow-lan: false
mode: rule
log-level: silent
tun:
  enable: false
proxy-providers:
  ${proxyProviderName}:
    type: file
    path: ${JSON.stringify(proxyFile)}
    health-check:
      enable: false
rule-providers:
  ${ruleProviderName}:
    type: file
    behavior: classical
    format: yaml
    path: ${JSON.stringify(ruleFile)}
proxy-groups:
  - name: E2E Providers Group
    type: select
    use:
      - ${proxyProviderName}
    proxies:
      - DIRECT
rules:
  - RULE-SET,${ruleProviderName},DIRECT
  - MATCH,DIRECT
`;

describe('main provider details with isolated native file providers', () => {
  let profileUid: string | undefined;
  let previousCurrent: string | null = null;
  let selectionMayHaveChanged = false;

  before(async () => {
    await browser.setWindowSize(1270, 700);
    await openMainRoute('/main/providers');

    assert.ok(
      process.env.CHIMERA_E2E_RUNTIME_DIR,
      'The provider fixture requires an isolated E2E runtime directory.',
    );
    // Core's config-check SAFE_PATHS explicitly include the isolated app data
    // directory, not arbitrary files under the temporary config directory.
    fs.mkdirSync(providerDir, { recursive: true });
    const proxyFile = path.join(providerDir, 'proxy.yaml');
    const ruleFile = path.join(providerDir, 'rules.yaml');
    fs.writeFileSync(
      proxyFile,
      `proxies:
  - name: ${proxyNodeName}
    type: socks5
    server: 127.0.0.1
    port: 65535
    udp: false
`,
    );
    fs.writeFileSync(
      ruleFile,
      'payload:\n  - DOMAIN-SUFFIX,isolated.example.invalid\n',
    );

    const initial = await readProfiles();
    previousCurrent = initial.current ?? null;
    selectionMayHaveChanged = true;
    const created = await createProfile(
      localConfigProfileRequest(profileName),
      renderFixture(proxyFile, ruleFile),
    );
    profileUid = created.value;
    committedValue(created, 'test provider Profile creation');

    const profiles = await readProfiles();
    profileUid ??= profiles.items.find((p) => p.name === profileName)?.uid;
    assert.ok(profileUid, 'Test provider Profile was not created.');

    if (profiles.current === profileUid) {
      const applied = created.commits.some(
        (receipt) => receipt.runtime === 'applied',
      );
      if (!applied) {
        assert.ok(
          created.commits.every(
            (receipt) =>
              receipt.runtime === 'unchanged' ||
              receipt.runtime === 'saved_inactive',
          ),
          'Unexpected no-Core Profile creation receipts.',
        );
        await invoke('restart_sidecar');
      }
    } else {
      appliedValue(
        await invoke('activate_profile', { uid: profileUid }),
        'test provider Profile activation',
      );
    }

    let lastStatus: unknown;
    try {
      await browser.waitUntil(
        async () => {
          const [status] =
            await invoke<[unknown, number, string]>('get_core_status');
          lastStatus = status;
          return status === 'Running';
        },
        {
          timeout: 30_000,
          timeoutMsg: 'Provider fixture Core did not reach Running.',
        },
      );
    } catch (cause) {
      throw new Error(
        `Provider fixture Core status: ${JSON.stringify(lastStatus)}`,
        {
          cause,
        },
      );
    }

    await browser.refresh();
    await openMainRoute('/main/providers');

    await browser.waitUntil(
      async () =>
        browser.execute(
          (names) =>
            names.every((name) =>
              Array.from(
                document.querySelectorAll<HTMLAnchorElement>(
                  'a[href^="/main/providers/"]',
                ),
              ).some((link) => link.textContent?.includes(name)),
            ),
          [proxyProviderName, ruleProviderName],
        ),
      {
        timeout: 30_000,
        timeoutMsg:
          'Both real native file providers were not visible in Providers.',
      },
    );
  });

  after(async () => {
    const actions = [
      {
        label: 'restore previous Profile selection',
        run: async () => {
          if (!selectionMayHaveChanged) return;
          committedValue(
            await invoke('activate_profile', { uid: previousCurrent }),
            'restore previous Profile',
          );
        },
      },
      {
        label: 'delete only test Profile',
        run: async () => {
          const uid =
            profileUid ??
            (await readProfiles()).items.find(
              (item) => item.name === profileName,
            )?.uid;
          if (uid) {
            committedValue(
              await invoke('delete_profile', { uid }),
              'delete test Profile',
            );
          }
        },
      },
      {
        label: 'remove only test-owned provider files',
        run: async () => {
          if (process.env.CHIMERA_E2E_RUNTIME_DIR) {
            fs.rmSync(providerDir, { recursive: true, force: true });
          }
        },
      },
    ];
    await runCleanupActions('Providers detail local fixture', actions);
  });

  it('opens a populated proxy provider detail showing the ref health-check action', async () => {
    await openMainRoute(
      `/main/providers/proxies/${encodeURIComponent(proxyProviderName)}`,
    );
    const card = await $('[data-slot="providers-proxy-info-card"]');
    await card.waitForDisplayed({ timeout: 15_000 });
    const update = await card.$('[data-slot="providers-detail-update"]');
    const healthcheck = await card.$(
      '[data-slot="providers-detail-healthcheck"]',
    );
    await update.waitForDisplayed({ timeout: 10_000 });
    await healthcheck.waitForDisplayed({ timeout: 10_000 });
    assert.equal(await healthcheck.isEnabled(), true);
    const locale = await browser.execute(() => document.documentElement.lang);
    assert.ok(locale, 'The interface locale must be defined.');
    // The canonical ref label is a *health check*, not a group delay test.
    const label = (await healthcheck.getText()).trim();
    const catalog = await readAppMessageCatalog();
    assert.equal(
      label,
      catalog.providers_healthcheck_provider,
      'The detail action must use the ref health-check translation, not the previous delay-test label.',
    );
    assert.notEqual(label, (await update.getText()).trim());
    const updated = await card.$('[data-slot="providers-detail-updated-at"]');
    await updated.waitForDisplayed({ timeout: 10_000 });
    assert.ok((await updated.getText()).length > 0);
    assert.match(await card.getText(), /1/);
  });

  it('opens a populated rule provider detail with a localized update timestamp', async () => {
    await openMainRoute(
      `/main/providers/rules/${encodeURIComponent(ruleProviderName)}`,
    );
    const card = await $('[data-slot="providers-rule-info-card"]');
    await card.waitForDisplayed({ timeout: 15_000 });
    const update = await card.$('[data-slot="providers-detail-update"]');
    await update.waitForDisplayed({ timeout: 10_000 });
    assert.equal(await update.isEnabled(), true);
    const updated = await card.$('[data-slot="providers-detail-updated-at"]');
    await updated.waitForDisplayed({ timeout: 10_000 });
    assert.ok((await updated.getText()).trim().length > 0);
    assert.equal(
      (await card.$$('[data-slot="providers-detail-healthcheck"]')).length,
      0,
    );
  });
});
