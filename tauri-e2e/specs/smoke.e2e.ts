import assert from 'node:assert/strict';

describe('Chimera desktop smoke test', () => {
  before(async () => {
    await browser.waitUntil(
      async () =>
        browser.execute(() => {
          const root = document.getElementById('root');
          return document.readyState === 'complete' && (root?.childElementCount ?? 0) > 0;
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
    assert.equal(url.hostname, 'tauri.localhost');
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

  it('navigates from the sidebar to settings', async () => {
    const settings = await $('//*[normalize-space()="设置"]');
    await settings.waitForClickable({ timeout: 15_000 });
    await settings.click();
    await browser.waitUntil(
      async () => browser.execute(() => window.location.pathname === '/settings'),
      { timeout: 15_000, timeoutMsg: 'Settings navigation did not complete.' },
    );
  });
});