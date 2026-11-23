import assert from 'node:assert/strict';
import { execFileSync, spawnSync } from 'node:child_process';
import fs from 'node:fs';
import { createServer, type Server } from 'node:http';
import path from 'node:path';
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

/**
 * Contract: real UI -> Native IPC -> live Mihomo HTTP provider download.
 * Each path has an isolated request counter and mutable response version.
 * A successful button click alone is not sufficient: verify a new HTTP
 * request, the Core's authoritative provider count, and the live UI count.
 */
const profileName = `E2E Remote Providers ${process.pid}-${Date.now()}`;
const proxyProviderName = 'e2e-http-proxy-provider';
const ruleProviderName = 'e2e-http-rule-provider';
const proxyNodeBase = 'E2E HTTP SOCKS';
const fixtureDir = path.join(
  process.env.CHIMERA_E2E_RUNTIME_DIR ?? '',
  'data',
  'remote-providers-fixture',
);

type NativeProxyProvider = { proxyCount: number };
type NativeRuleProvider = { ruleCount: number | null };
type NativeRuleProviderResponse = {
  providers: Record<string, NativeRuleProvider>;
};

const requestCounts = { proxies: 0, rules: 0 };
let proxyVersion = 1;
let ruleVersion = 1;
let proxyHttpStatus = 200;
let server: Server | null = null;

const proxyYaml = (count: number) =>
  `proxies:\n${Array.from(
    { length: count },
    (_, index) => `  - name: ${proxyNodeBase} ${index + 1}
    type: socks5
    server: 127.0.0.1
    port: 65535
    udp: false
`,
  ).join('')}`;
const ruleYaml = (count: number) =>
  `payload:\n${Array.from(
    { length: count },
    (_, index) => `  - DOMAIN-SUFFIX,e2e-${index + 1}.invalid\n`,
  ).join('')}`;

function fixture(port: number, proxyPath: string, rulePath: string) {
  return `mixed-port: 27896
allow-lan: false
mode: rule
log-level: silent
tun:
  enable: false
proxy-providers:
  ${proxyProviderName}:
    type: http
    url: "http://127.0.0.1:${port}/proxies.yaml"
    interval: 86400
    path: ${JSON.stringify(proxyPath)}
    health-check:
      enable: false
rule-providers:
  ${ruleProviderName}:
    type: http
    url: "http://127.0.0.1:${port}/rules.yaml"
    interval: 86400
    behavior: classical
    format: yaml
    path: ${JSON.stringify(rulePath)}
proxy-groups:
  - name: E2E HTTP Provider Group
    type: select
    use:
      - ${proxyProviderName}
    proxies:
      - DIRECT
rules:
  - RULE-SET,${ruleProviderName},DIRECT
  - MATCH,DIRECT
`;
}

async function closeServer() {
  if (!server?.listening) return;
  await new Promise<void>((resolve, reject) =>
    server?.close((error) => (error ? reject(error) : resolve())),
  );
}

async function proxyCount(): Promise<number | undefined> {
  const providers = await invoke<Record<string, NativeProxyProvider>>(
    'clash_api_get_providers_proxies',
  );
  return providers[proxyProviderName]?.proxyCount;
}

async function ruleCount(): Promise<number | null | undefined> {
  const providers = await invoke<NativeRuleProviderResponse>(
    'clash_api_get_providers_rules',
  );
  return providers.providers[ruleProviderName]?.ruleCount;
}

async function waitForRealRefresh(
  kind: keyof typeof requestCounts,
  previousCount: number,
  currentCount: () => Promise<number | null | undefined>,
  expected: number,
) {
  let lastObserved: unknown;
  try {
    await browser.waitUntil(
      async () => {
        const observedCount = await currentCount();
        lastObserved = {
          requests: requestCounts[kind],
          observedCount,
        };
        return (
          requestCounts[kind] > previousCount && observedCount === expected
        );
      },
      { timeout: 30_000, timeoutMsg: `${kind} update was not verified.` },
    );
  } catch (cause) {
    throw new Error(
      `${kind} did not fetch new content and expose count ${expected}: ${JSON.stringify(lastObserved)}`,
      { cause },
    );
  }
}

// Tauri message dialogs are OS-native and not in WebDriver's DOM. Only
// Linux/X11 with an actual xdotool binary has a proven dialog driver here;
// on other platforms success-path checks still run but this negative case
// is explicitly skipped until its native dialog driver is implemented.
const hasNativeDialogDriver =
  process.platform === 'linux' &&
  Boolean(process.env.DISPLAY) &&
  spawnSync('xdotool', ['--version'], { timeout: 1_500 }).status === 0;

async function acknowledgeNativeProviderErrorDialog(): Promise<void> {
  let dialogWindows: string[] = [];
  await browser.waitUntil(
    async () => {
      try {
        const output = execFileSync(
          'xdotool',
          ['search', '--onlyvisible', '--name', 'Clash Chimera.*Error'],
          { encoding: 'utf8', timeout: 1_500 },
        );
        dialogWindows = output.trim().split('\n').filter(Boolean);
        return dialogWindows.length > 0;
      } catch {
        return false;
      }
    },
    {
      timeout: 15_000,
      timeoutMsg: 'A failed provider update showed no native error dialog.',
    },
  );
  assert.equal(dialogWindows.length, 1);
  const title = execFileSync('xdotool', ['getwindowname', dialogWindows[0]], {
    encoding: 'utf8',
    timeout: 2_000,
  }).trim();
  assert.match(title, /Clash Chimera.*Error/);
  // Explicit focus is required in Xvfb without a window manager; sending a
  // key --window to an unfocused GTK dialog does not acknowledge it.
  execFileSync('xdotool', ['windowfocus', '--sync', dialogWindows[0]], {
    timeout: 2_000,
  });
  execFileSync('xdotool', ['key', '--clearmodifiers', 'Return'], {
    timeout: 2_000,
  });
}

describe('main Providers update buttons download real isolated HTTP resources', () => {
  let profileUid: string | undefined;
  let previousCurrent: string | null = null;
  let selectionMayHaveChanged = false;

  before(async () => {
    await browser.setWindowSize(1270, 700);
    await openMainRoute('/main/providers');

    assert.ok(process.env.CHIMERA_E2E_RUNTIME_DIR);
    fs.mkdirSync(fixtureDir, { recursive: true });
    server = createServer((request, response) => {
      const pathname = new URL(request.url ?? '/', 'http://127.0.0.1').pathname;
      let body: string;
      if (pathname === '/proxies.yaml') {
        requestCounts.proxies++;
        body = proxyYaml(proxyVersion);
      } else if (pathname === '/rules.yaml') {
        requestCounts.rules++;
        body = ruleYaml(ruleVersion);
      } else {
        response.writeHead(404);
        response.end('Unknown fixture URL');
        return;
      }
      response.writeHead(pathname === '/proxies.yaml' ? proxyHttpStatus : 200, {
        'content-type': 'application/yaml',
        'cache-control': 'no-store',
        'content-length': String(Buffer.byteLength(body)),
      });
      response.end(body);
    });
    await new Promise<void>((resolve, reject) => {
      server?.once('error', reject);
      server?.listen(0, '127.0.0.1', resolve);
    });
    const address = server.address();
    assert.ok(address && typeof address !== 'string');

    const profiles = await readProfiles();
    previousCurrent = profiles.current ?? null;
    selectionMayHaveChanged = true;
    const created = await createProfile(
      localConfigProfileRequest(profileName),
      fixture(
        address.port,
        path.join(fixtureDir, 'proxies.yaml'),
        path.join(fixtureDir, 'rules.yaml'),
      ),
    );
    profileUid = created.value;
    committedValue(created, 'isolated HTTP Provider Profile creation');

    const current = await readProfiles();
    profileUid ??= current.items.find((p) => p.name === profileName)?.uid;
    assert.ok(profileUid, 'The HTTP Provider Profile was not persisted.');
    if (current.current === profileUid) {
      if (!created.commits.some((receipt) => receipt.runtime === 'applied')) {
        assert.ok(
          created.commits.every(
            (receipt) =>
              receipt.runtime === 'unchanged' ||
              receipt.runtime === 'saved_inactive',
          ),
        );
        await invoke('restart_sidecar');
      }
    } else {
      appliedValue(
        await invoke('activate_profile', { uid: profileUid }),
        'activate isolated HTTP Provider Profile',
      );
    }
    let lastState: unknown;
    try {
      await browser.waitUntil(
        async () => {
          const [status] =
            await invoke<[unknown, number, string]>('get_core_status');
          lastState = status;
          return status === 'Running';
        },
        {
          timeout: 30_000,
          timeoutMsg: 'The HTTP Provider fixture Core is not running.',
        },
      );
    } catch (cause) {
      throw new Error(
        `HTTP Provider Core did not start: ${String(lastState)}`,
        { cause },
      );
    }

    // Core must really load both resources before any UI update, which
    // prevents an unrelated Core failure from looking like a failed update.
    await browser.waitUntil(
      async () => (await proxyCount()) === 1 && (await ruleCount()) === 1,
      {
        timeout: 30_000,
        timeoutMsg: 'The initial HTTP Providers did not load into live Core.',
      },
    );
    assert.ok(requestCounts.proxies >= 1);
    assert.ok(requestCounts.rules >= 1);

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
        timeoutMsg: 'Loaded HTTP Providers were not visible in Main UI.',
      },
    );
  });

  after(async () => {
    await runCleanupActions('isolated HTTP provider update', [
      {
        label: 'restore previous Profile selection',
        run: async () => {
          if (!selectionMayHaveChanged) return;
          committedValue(
            await invoke('activate_profile', { uid: previousCurrent }),
            'restore previous selection',
          );
        },
      },
      {
        label: 'delete owned Profile',
        run: async () => {
          const uid =
            profileUid ??
            (await readProfiles()).items.find((p) => p.name === profileName)
              ?.uid;
          if (uid) {
            committedValue(
              await invoke('delete_profile', { uid }),
              'delete isolated HTTP Provider Profile',
            );
          }
        },
      },
      {
        label: 'close owned HTTP resource server',
        run: closeServer,
      },
      {
        label: 'remove owned data fixture files',
        run: async () => {
          if (process.env.CHIMERA_E2E_RUNTIME_DIR) {
            fs.rmSync(fixtureDir, { recursive: true, force: true });
          }
        },
      },
    ]);
  });

  it('updates the proxy provider through the visible detail button and refreshes the UI count', async () => {
    await openMainRoute(
      `/main/providers/proxies/${encodeURIComponent(proxyProviderName)}`,
    );
    const action = await $('[data-slot="providers-detail-update"]');
    await action.waitForClickable({ timeout: 15_000 });
    assert.equal(await proxyCount(), 1);
    const previous = requestCounts.proxies;
    proxyVersion = 2;
    await action.click();
    await waitForRealRefresh('proxies', previous, proxyCount, 2);
    await browser.waitUntil(
      async () =>
        browser.execute(() => {
          const card = document.querySelector<HTMLElement>(
            '[data-slot="providers-proxy-info-card"]',
          );
          return (
            card?.textContent?.includes('2') &&
            card
              ?.querySelector('[data-slot="providers-detail-update"]')
              ?.getAttribute('data-error') === 'false'
          );
        }),
      {
        timeout: 15_000,
        timeoutMsg: 'The proxy provider detail did not show the new count.',
      },
    );
  });

  it('updates one rule provider from the listing without navigating into the detail', async () => {
    await openMainRoute('/main/providers');
    const action = await $(
      `[data-slot="providers-item-update"][data-provider-name="${ruleProviderName}"]`,
    );
    await action.waitForClickable({ timeout: 15_000 });
    assert.equal(await ruleCount(), 1);
    const previous = requestCounts.rules;
    ruleVersion = 2;
    await action.click();
    assert.equal(new URL(await browser.getUrl()).pathname, '/main/providers');
    await waitForRealRefresh('rules', previous, ruleCount, 2);
    await browser.waitUntil(
      async () =>
        (await action.getAttribute('data-error')) === 'false' &&
        (await action.getAttribute('data-loading')) !== 'true',
      {
        timeout: 15_000,
        timeoutMsg: 'The rule list update did not settle successfully.',
      },
    );
  });

  it('updates each non-empty group through its visible bulk button', async () => {
    await openMainRoute('/main/providers');
    const proxyAction = await $('[data-slot="providers-update-all-proxies"]');
    const ruleAction = await $('[data-slot="providers-update-all-rules"]');
    await proxyAction.waitForClickable({ timeout: 15_000 });
    await ruleAction.waitForClickable({ timeout: 15_000 });

    proxyVersion = 3;
    const beforeProxy = requestCounts.proxies;
    await proxyAction.click();
    await waitForRealRefresh('proxies', beforeProxy, proxyCount, 3);
    assert.equal(await proxyAction.getAttribute('data-error'), 'false');

    ruleVersion = 3;
    const beforeRule = requestCounts.rules;
    await ruleAction.click();
    await waitForRealRefresh('rules', beforeRule, ruleCount, 3);
    assert.equal(await ruleAction.getAttribute('data-error'), 'false');
    assert.equal(new URL(await browser.getUrl()).pathname, '/main/providers');
  });

  it('keeps the last successful proxy count when an update receives HTTP 503', async function () {
    if (!hasNativeDialogDriver) this.skip();
    await openMainRoute(
      `/main/providers/proxies/${encodeURIComponent(proxyProviderName)}`,
    );
    const action = await $('[data-slot="providers-detail-update"]');
    await action.waitForClickable({ timeout: 15_000 });
    assert.equal(await proxyCount(), 3);
    assert.equal(await action.getAttribute('data-error'), 'false');
    const before = requestCounts.proxies;
    proxyHttpStatus = 503;

    try {
      await action.click();
      await browser.waitUntil(async () => requestCounts.proxies > before, {
        timeout: 20_000,
        timeoutMsg: 'The failed UI action never requested the HTTP resource.',
      });

      await acknowledgeNativeProviderErrorDialog();
      await browser.waitUntil(
        async () => (await action.getAttribute('data-error')) === 'true',
        {
          timeout: 10_000,
          timeoutMsg:
            'The rejected HTTP provider update was marked successful.',
        },
      );
      assert.equal(await proxyCount(), 3);
      assert.equal(
        new URL(await browser.getUrl()).pathname,
        `/main/providers/proxies/${proxyProviderName}`,
      );
    } finally {
      proxyHttpStatus = 200;
    }
  });

  it('marks bulk update as failed while preserving the last accepted Core provider', async function () {
    if (!hasNativeDialogDriver) this.skip();

    await openMainRoute('/main/providers');
    const action = await $('[data-slot="providers-update-all-proxies"]');
    await action.waitForClickable({ timeout: 15_000 });
    assert.equal(await action.getAttribute('data-error'), 'false');
    assert.equal(await proxyCount(), 3);
    const before = requestCounts.proxies;
    proxyHttpStatus = 503;
    try {
      await action.click();
      await browser.waitUntil(async () => requestCounts.proxies > before, {
        timeout: 20_000,
        timeoutMsg: 'Bulk refresh did not fetch its HTTP resource.',
      });
      await acknowledgeNativeProviderErrorDialog();
      await browser.waitUntil(
        async () => (await action.getAttribute('data-error')) === 'true',
        {
          timeout: 10_000,
          timeoutMsg: 'Bulk provider failure was marked successful.',
        },
      );
      assert.equal(await proxyCount(), 3);
      assert.equal(new URL(await browser.getUrl()).pathname, '/main/providers');
    } finally {
      proxyHttpStatus = 200;
    }
  });
});
