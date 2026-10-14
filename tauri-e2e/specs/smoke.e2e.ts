import assert from 'node:assert/strict';
import { openMainRoute } from './main-window.js';

async function assertConnectSourcesAreAvailable(windowName: string) {
  const result = await browser.execute(async () => {
    const readResponse = async (url: string) => {
      try {
        const response = await fetch(url, {
          cache: 'no-store',
          signal: AbortSignal.timeout(5_000),
        });
        return {
          status: response.status,
          ok: response.ok,
          body: (await response.text()).slice(0, 80),
        };
      } catch (error) {
        return { error: String(error) };
      }
    };

    return {
      sameOrigin: await readResponse(window.location.href),
      data: await readResponse('data:text/plain,chimera-csp-probe'),
    };
  });

  if ('error' in result.sameOrigin) {
    assert.fail(
      `${windowName} same-origin request was blocked: ${result.sameOrigin.error}`,
    );
  }
  assert.equal(result.sameOrigin.ok, true);
  if ('error' in result.data) {
    assert.fail(
      `${windowName} data URL request was blocked: ${result.data.error}`,
    );
  }
  assert.equal(result.data.status, 200);
  assert.equal(result.data.body, 'chimera-csp-probe');
}

describe('Chimera desktop smoke test', () => {
  before(async () => {
    await browser.waitUntil(
      async () =>
        browser.execute(() => {
          const root = document.getElementById('root');
          return (
            document.readyState === 'complete' &&
            (root?.childElementCount ?? 0) > 0
          );
        }),
      { timeout: 30_000, timeoutMsg: 'The Chimera frontend did not render.' },
    );
  });

  it('loads the bundled frontend', async () => {
    const state = await browser.execute(() => ({
      href: window.location.href,
      title: document.title,
      rootExists: document.getElementById('root') !== null,
    }));

    const url = new URL(state.href);
    const expectedHostname =
      process.platform === 'darwin' ? 'localhost' : 'tauri.localhost';
    assert.equal(url.hostname, expectedHostname);
    assert.notEqual(url.protocol, 'chrome-error:');
    assert.equal(state.title, 'Clash Chimera');
    assert.equal(state.rootExists, true);
  });

  it('renders application content', async () => {
    const state = await browser.execute(() => ({
      rootChildCount: document.getElementById('root')?.childElementCount ?? 0,
      bodyText: document.body?.innerText.trim() ?? '',
    }));

    assert.ok(state.rootChildCount > 0);
    assert.ok(state.bodyText.length > 0);
  });

  it('loads the bundled country flag emoji font', async () => {
    const state = await browser.execute(async () => {
      const flag = String.fromCodePoint(0x1f1fa, 0x1f1f8);
      const family = 'Color Emoji Flags';
      const registeredFamilies = Array.from(document.fonts).map(
        (font) => font.family,
      );

      await document.fonts.load(`48px "${family}"`, flag);

      return {
        isRegistered: registeredFamilies.includes(family),
        isLoaded: document.fonts.check(`48px "${family}"`, flag),
      };
    });

    assert.equal(state.isRegistered, true);
    assert.equal(state.isLoaded, true);
  });

  it('navigates from the sidebar to settings', async () => {
    const settings = await $('[data-testid="sidebar-route-settings"]');
    await settings.waitForClickable({ timeout: 15_000 });
    await settings.click();
    await browser.waitUntil(
      async () =>
        browser.execute(() => window.location.pathname === '/settings'),
      { timeout: 15_000, timeoutMsg: 'Settings navigation did not complete.' },
    );
  });

  // Contract: from each supported desktop window, same-origin and data: fetches
  // complete and return readable responses. The previous connect-src blocked
  // both requests; this probes the real WebView policy without external network.
  it('allows same-origin and data URL requests in both desktop windows', async () => {
    await browser.switchToWindow('legacy');
    await assertConnectSourcesAreAvailable('Legacy');

    await openMainRoute('/main/dashboard');
    await assertConnectSourcesAreAvailable('Main');
  });
});
