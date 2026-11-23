import assert from 'node:assert/strict';
import { createServer, type Server } from 'node:http';
import { connect, type Socket } from 'node:net';
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

// Real loopback HTTP traffic is proxied through the isolated running Mihomo
// core. The connection must remain open while the native on-demand detail
// Channel and virtual table render it. No mocked UI state or injected rows.
const profileName = 'E2E Live Connections ' + process.pid + '-' + Date.now();
const mixedPort = 27897;
const fixture = [
  'mixed-port: ' + mixedPort,
  'allow-lan: false',
  'mode: rule',
  'log-level: silent',
  'tun:',
  '  enable: false',
  'proxies: []',
  'proxy-groups: []',
  'rules:',
  '  - MATCH,DIRECT',
  '',
].join('\n');
let httpServer: Server | undefined;
const ownedSockets = new Set<Socket>();
let coreMixedPort = 0;
let streamTargetPort = 0;
let profileUid: string | undefined;
let previousCurrent: string | null = null;
let selectionMayHaveChanged = false;
let upstreamRequests = 0;
let upstreamClosed = 0;

const liveRowSelector =
  '[data-slot="connections-virtual-tbody"] tr:not([aria-hidden="true"])';

async function waitForLiveRow() {
  let observed = 'not read';
  try {
    await browser.waitUntil(
      async () => {
        observed = await browser.execute(
          (selector) =>
            Array.from(document.querySelectorAll<HTMLTableRowElement>(selector))
              .map((row) => row.innerText)
              .join(' | '),
          liveRowSelector,
        );
        return upstreamRequests > 0 && observed.includes('127.0.0.1');
      },
      {
        timeout: 30_000,
        interval: 400,
        timeoutMsg: 'A live Core connection was not shown in Main Connections.',
      },
    );
  } catch (cause) {
    throw new Error(
      'Loopback HTTP requests=' +
        upstreamRequests +
        ', closed=' +
        upstreamClosed +
        ', live rows=' +
        observed,
      { cause },
    );
  }
}

async function openProxyConnection(upstreamPort: number, listenPort: number) {
  // The managed Clash Guard may override the Profile's mixed-port. Use the
  // effective live Core port, not the requested Profile YAML value.
  let client: Socket | undefined;
  let lastError: unknown;
  for (let attempt = 0; attempt < 30; attempt++) {
    client = connect({ host: '127.0.0.1', port: listenPort });
    try {
      await new Promise<void>((resolve, reject) => {
        client?.once('connect', resolve);
        client?.once('error', reject);
      });
      break;
    } catch (error) {
      lastError = error;
      client.destroy();
      client = undefined;
      await new Promise((resolve) => setTimeout(resolve, 200));
    }
  }
  assert.ok(
    client,
    'Effective mixed-port never listened: ' +
      listenPort +
      ', ' +
      String(lastError),
  );
  ownedSockets.add(client);
  client.on('error', () => {
    // Core closes the test socket after the user terminates the connection.
  });
  client.write(
    'GET http://127.0.0.1:' +
      upstreamPort +
      '/stream HTTP/1.1\r\n' +
      'Host: 127.0.0.1:' +
      upstreamPort +
      '\r\n' +
      'Accept: */*\r\n' +
      'Connection: keep-alive\r\n\r\n',
  );
}

describe('Main Connections live native virtual table and row actions', () => {
  before(async () => {
    await browser.setWindowSize(1240, 638);
    await openMainRoute('/main/connections');

    httpServer = createServer((request, response) => {
      if (request.url !== '/stream') {
        response.writeHead(404);
        response.end('not found');
        return;
      }
      upstreamRequests++;
      response.writeHead(200, {
        'content-type': 'text/plain',
        'cache-control': 'no-store',
      });
      response.write('stream-ready\n');
      const ticker = setInterval(() => {
        if (!response.destroyed) response.write('alive\n');
      }, 1000);
      response.on('close', () => {
        upstreamClosed++;
        clearInterval(ticker);
      });
    });
    const server = httpServer;
    await new Promise<void>((resolve, reject) => {
      server.once('error', reject);
      server.listen(0, '127.0.0.1', resolve);
    });
    const address = server.address();
    assert.ok(address && typeof address !== 'string');

    const initial = await readProfiles();
    previousCurrent = initial.current ?? null;
    selectionMayHaveChanged = true;
    const created = await createProfile(
      localConfigProfileRequest(profileName),
      fixture,
    );
    profileUid = created.value;
    committedValue(created, 'create loopback Connections Profile');
    const current = await readProfiles();
    profileUid ??= current.items.find(
      (profile) => profile.name === profileName,
    )?.uid;
    assert.ok(
      profileUid,
      'The loopback Connections Profile was not persisted.',
    );
    if (current.current === profileUid) {
      if (!created.commits.some((commit) => commit.runtime === 'applied')) {
        assert.ok(
          created.commits.every(
            (commit) =>
              commit.runtime === 'unchanged' ||
              commit.runtime === 'saved_inactive',
          ),
          'Core profile creation did not settle safely.',
        );
        await invoke('restart_sidecar');
      }
    } else {
      appliedValue(
        await invoke('activate_profile', { uid: profileUid }),
        'activate loopback Connections Profile',
      );
    }

    await browser.waitUntil(
      async () => {
        const [status] =
          await invoke<[string, number, string]>('get_core_status');
        return status === 'Running';
      },
      {
        timeout: 30_000,
        timeoutMsg: 'The isolated Connections Core did not reach Running.',
      },
    );

    // Remount the Main route after the profile switch, then keep the actual
    // proxy-to-upstream socket alive across all subsequent UI operations.
    await browser.refresh();
    await openMainRoute('/main/connections');
    const active = await invoke<{ 'mixed-port': number | null }>(
      'clash_api_get_configs',
    );
    const activePort = active['mixed-port'];
    assert.ok(
      typeof activePort === 'number' && activePort > 0,
      'The live Core did not expose its effective mixed-port: ' +
        JSON.stringify(active),
    );
    coreMixedPort = activePort;
    streamTargetPort = address.port;
    await openProxyConnection(streamTargetPort, coreMixedPort);
    await waitForLiveRow();
  });

  after(async () => {
    await runCleanupActions('isolated live Connections profile', [
      {
        label: 'release only the test HTTP clients',
        run: async () => {
          for (const client of ownedSockets) client.destroy();
          ownedSockets.clear();
        },
      },
      {
        label: 'restore previous active Profile',
        run: async () => {
          if (!selectionMayHaveChanged) return;
          committedValue(
            await invoke('activate_profile', { uid: previousCurrent }),
            'restore previous Profile',
          );
        },
      },
      {
        label: 'delete only owned test Profile',
        run: async () => {
          const uid =
            profileUid ??
            (await readProfiles()).items.find(
              (item) => item.name === profileName,
            )?.uid;
          if (uid) {
            committedValue(
              await invoke('delete_profile', { uid }),
              'delete loopback Connections Profile',
            );
          }
        },
      },
      {
        label: 'close test HTTP server',
        run: async () => {
          if (httpServer?.listening) {
            httpServer.closeAllConnections();
            await new Promise<void>((resolve, reject) =>
              httpServer?.close((error) => (error ? reject(error) : resolve())),
            );
          }
        },
      },
    ]);
  });

  it('renders a real proxied connection with the reference physical virtual rows', async () => {
    await waitForLiveRow();
    const values = await browser.execute((selector) => {
      const row = document.querySelector<HTMLTableRowElement>(selector);
      const table = document.querySelector<HTMLTableElement>(
        '[data-slot="connections-virtual-table"]',
      );
      const virtualContainer = document.querySelector<HTMLElement>(
        '[data-slot="connections-virtual-container"]',
      );
      const tbody = document.querySelector<HTMLElement>(
        '[data-slot="connections-virtual-tbody"]',
      );
      const header = table?.querySelector('th');
      const rect = (item: Element | null | undefined) =>
        item?.getBoundingClientRect().height ?? 0;
      return {
        rowText: row?.innerText ?? '',
        rowHeight: rect(row),
        rowTransform: row ? getComputedStyle(row).transform : null,
        headerHeight: rect(header),
        spacerCount:
          tbody?.querySelectorAll('tr[aria-hidden="true"]').length ?? 0,
        tableHeight: rect(table),
        containerHeight: rect(virtualContainer),
      };
    }, liveRowSelector);
    assert.match(values.rowText, /127\.0\.0\.1/);
    assert.ok(Math.abs(values.rowHeight - 44) <= 1, JSON.stringify(values));
    assert.ok(Math.abs(values.headerHeight - 40) <= 1, JSON.stringify(values));
    assert.equal(values.rowTransform, 'none', JSON.stringify(values));
    assert.equal(values.spacerCount, 2, JSON.stringify(values));
    assert.ok(values.tableHeight >= values.headerHeight + values.rowHeight - 1);
    assert.ok(
      values.containerHeight >= values.headerHeight + values.rowHeight - 1,
    );
  });

  it('filters a live Core connection then restores it through the ref SearchField', async () => {
    const search = await $('[data-slot="connections-search"]');
    await search.waitForDisplayed();
    await search.setValue('__no_live_connection_matches_this__');
    await $('[data-slot="connections-no-connections"]').waitForDisplayed({
      timeout: 15_000,
    });
    const clear = await $(
      '[data-slot="data-page-dock"] [data-slot="search-field-clear"]',
    );
    await clear.waitForClickable({ timeout: 10_000 });
    await clear.click();
    await waitForLiveRow();
    assert.equal(await search.getValue(), '');
  });

  it('opens real connection details from the context menu and closes the native connection', async () => {
    const row = await $(liveRowSelector);
    await row.waitForDisplayed({ timeout: 10_000 });
    // WebDriver's doubleClick did not emit a DOM dblclick in this WebKitGTK
    // runner. Use the ref's other real user gesture: right-click and select
    // the visible View Details context-menu item, without synthetic events.
    await row.click({ button: 'right' });
    const menu = await $('[role="menu"][data-state="open"]');
    await menu.waitForDisplayed({ timeout: 10_000 });
    const catalog = await readAppMessageCatalog();
    const viewLabel = catalog.connections_view_details;
    assert.equal(typeof viewLabel, 'string');
    const menuItems = await menu.$$('[role="menuitem"]');
    const menuTexts: string[] = [];
    for (const item of menuItems) {
      menuTexts.push((await item.getText()).trim());
    }
    const openItem =
      menuItems[menuTexts.findIndex((label) => label === viewLabel)];
    assert.ok(
      openItem,
      'The real connection context menu has no View Details action: ' +
        JSON.stringify(menuTexts),
    );
    await openItem.click();
    const modal = await $('[data-slot="modal-content"]');
    await modal.waitForDisplayed({ timeout: 10_000 });
    assert.match(await modal.getText(), /127\.0\.0\.1/);
    const messages = await readAppMessageCatalog();
    const text = messages.connections_close_connection;
    assert.equal(typeof text, 'string');
    const buttons = await modal.$$('button');
    const titles: string[] = [];
    for (const button of buttons) {
      titles.push(await button.getText());
    }
    const button = buttons[titles.findIndex((title) => title.trim() === text)];
    assert.ok(
      button,
      'No native close action in live connection details: ' +
        JSON.stringify(titles),
    );
    await button.click();
    await modal.waitForDisplayed({ reverse: true, timeout: 10_000 });
    await browser.waitUntil(
      async () =>
        upstreamClosed > 0 &&
        (await browser.execute(
          (selector) =>
            !Array.from(
              document.querySelectorAll<HTMLTableRowElement>(selector),
            ).some((item) => item.innerText.includes('127.0.0.1')),
          liveRowSelector,
        )),
      {
        timeout: 20_000,
        timeoutMsg:
          'Closing the connection did not close its stream and remove it from the table.',
      },
    );
    assert.ok(upstreamRequests >= 1);
  });

  it('virtualizes many real streams with physical spacer rows and closes all from the floating dock', async () => {
    const initialRequests = upstreamRequests;
    const burstSize = 48;
    // Separate TCP clients all go through the actual managed Core listener,
    // so the virtual list must render a subset rather than 48 DOM rows.
    for (let index = 0; index < burstSize; index++) {
      await openProxyConnection(streamTargetPort, coreMixedPort);
    }
    await browser.waitUntil(
      async () => upstreamRequests >= initialRequests + burstSize,
      {
        timeout: 30_000,
        timeoutMsg:
          'The streaming server did not receive all 48 Mihomo-proxied requests.',
      },
    );

    const viewportSelector =
      '[data-slot="connections-scroll-wrapper"] [data-slot="scroll-area-viewport"]';
    const viewport = await $(viewportSelector);
    await viewport.waitForDisplayed({ timeout: 15_000 });
    await browser.waitUntil(
      async () =>
        browser.execute((selector) => {
          const el = document.querySelector<HTMLElement>(selector);
          return !!el && el.scrollHeight - el.clientHeight > 600;
        }, viewportSelector),
      {
        timeout: 20_000,
        timeoutMsg: 'The non-empty virtual table never exceeded its viewport.',
      },
    );

    // Scroll the real WebKit viewport, not an injected connection model.
    await browser.execute((selector) => {
      const el = document.querySelector<HTMLElement>(selector);
      if (el) el.scrollTop = el.scrollHeight;
    }, viewportSelector);
    let latestState: unknown;
    await browser.waitUntil(
      async () => {
        const state = await browser.execute(() => {
          const tbody = document.querySelector<HTMLElement>(
            '[data-slot="connections-virtual-tbody"]',
          );
          const top = tbody?.querySelector<HTMLElement>(
            'tr[aria-hidden="true"] td',
          );
          const bottom = tbody?.querySelector<HTMLElement>(
            'tr[aria-hidden="true"]:last-child td',
          );
          const rendered =
            tbody?.querySelectorAll('tr:not([aria-hidden="true"])').length ?? 0;
          return {
            rendered,
            topSpacer: Number.parseFloat(top?.style.height || '0'),
            bottomSpacer: Number.parseFloat(bottom?.style.height || '0'),
          };
        });
        latestState = state;
        return (
          state.rendered > 0 &&
          state.rendered < burstSize &&
          state.topSpacer > 100 &&
          state.bottomSpacer >= 0
        );
      },
      {
        timeout: 20_000,
        timeoutMsg:
          'The reference physical spacer rows did not follow the real WebKit scroll.',
      },
    );
    assert.ok(latestState);

    const closeAll = await $('[data-slot="connections-close-all"]');
    await closeAll.waitForClickable({ timeout: 15_000 });
    const shouldClose = upstreamRequests;
    await closeAll.click();
    await browser.waitUntil(
      async () =>
        upstreamClosed >= shouldClose &&
        (await browser.execute(
          (selector) => document.querySelectorAll(selector).length === 0,
          liveRowSelector,
        )),
      {
        timeout: 25_000,
        timeoutMsg:
          'Closing all connections from the floating dock did not stop the real streams.',
      },
    );
    assert.equal(new URL(await browser.getUrl()).pathname, '/main/connections');
  });

  it('shows recorded real connections in Closed and All without destructive historical actions', async () => {
    type Historical = { id: string; dimensions: { target: string } };
    type Page = { connections: Historical[]; next: unknown };
    let recorded: Historical[] = [];
    await browser.waitUntil(
      async () => {
        const page = await invoke<Page>('query_traffic_closed_connections', {
          range: 'all',
          filters: [],
          before: null,
          limit: 100,
        });
        recorded = page.connections;
        return recorded.some((row) =>
          row.dimensions.target.includes('127.0.0.1'),
        );
      },
      {
        timeout: 25_000,
        timeoutMsg:
          'The native traffic store never recorded the closed HTTP streams.',
      },
    );

    const closedTab = await $(
      '[data-slot="connections-scope-tab"][data-scope="closed"]',
    );
    await closedTab.waitForClickable({ timeout: 10_000 });
    await closedTab.click();
    await browser.waitUntil(
      async () =>
        browser.execute(
          (selector) =>
            location.search.includes('scope=closed') &&
            Array.from(
              document.querySelectorAll<HTMLTableRowElement>(selector),
            ).some((row) => row.innerText.includes('127.0.0.1')),
          liveRowSelector,
        ),
      {
        timeout: 20_000,
        timeoutMsg: 'Closed tab did not show an actual stored connection.',
      },
    );

    const historicalRow = await $(liveRowSelector);
    await historicalRow.click({ button: 'right' });
    const menu = await $('[role="menu"][data-state="open"]');
    await menu.waitForDisplayed({ timeout: 10_000 });
    const messages = await readAppMessageCatalog();
    const viewLabel = messages.connections_view_details;
    const closeLabel = messages.connections_close_connection;
    if (typeof viewLabel !== 'string' || typeof closeLabel !== 'string') {
      throw new Error(
        'The current locale lacks connection detail action labels.',
      );
    }
    const menuItems = await menu.$$('[role="menuitem"]');
    const menuLabels: string[] = [];
    for (const item of menuItems) {
      menuLabels.push((await item.getText()).trim());
    }
    assert.ok(menuLabels.includes(viewLabel));
    assert.ok(
      !menuLabels.includes(closeLabel),
      'Closed record offered a live Core action.',
    );
    await menuItems[menuLabels.indexOf(viewLabel)].click();

    const modal = await $('[data-slot="modal-content"]');
    await modal.waitForDisplayed({ timeout: 10_000 });
    assert.match(await modal.getText(), /127\.0\.0\.1/);
    const modalButtons = await modal.$$('button');
    const buttonLabels: string[] = [];
    for (const item of modalButtons) {
      buttonLabels.push((await item.getText()).trim());
    }
    assert.ok(
      !buttonLabels.includes(closeLabel),
      'Historical detail offered Close Connection.',
    );
    const dismiss =
      modalButtons[buttonLabels.indexOf(String(messages.common_close))];
    assert.ok(dismiss, 'Historical detail cannot be dismissed.');
    await dismiss.click();
    await modal.waitForDisplayed({ reverse: true, timeout: 10_000 });

    const allTab = await $(
      '[data-slot="connections-scope-tab"][data-scope="all"]',
    );
    await allTab.waitForClickable({ timeout: 10_000 });
    await allTab.click();
    await browser.waitUntil(
      async () =>
        browser.execute(
          (selector) =>
            location.search.includes('scope=all') &&
            Array.from(
              document.querySelectorAll<HTMLTableRowElement>(selector),
            ).some((row) => row.innerText.includes('127.0.0.1')),
          liveRowSelector,
        ),
      {
        timeout: 20_000,
        timeoutMsg: 'All tab omitted the stored closed connection.',
      },
    );
    assert.ok(recorded.length > 0);
  });
});
