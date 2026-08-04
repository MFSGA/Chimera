import assert from 'node:assert/strict';

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
      async () =>
        browser.execute(() => window.location.pathname === '/settings'),
      { timeout: 15_000, timeoutMsg: 'Settings navigation did not complete.' },
    );
  });

  it('cancels a local profile draft without mutating isolated state', async () => {
    const target = new URL('/main/profiles/profile', await browser.getUrl())
      .href;
    await browser.url(target);
    await browser.waitUntil(
      async () =>
        browser.execute(() => {
          const text = document.body?.innerText ?? '';
          return (
            window.location.pathname.replace(/\/$/, '') ===
              '/main/profiles/profile' &&
            text.includes('没有找到任何配置，请尝试导入或创建配置。') &&
            Boolean(
              document.querySelector('[data-slot="profile-import-button"]'),
            )
          );
        }),
      {
        timeout: 30_000,
        timeoutMsg: 'The isolated profile list did not become ready.',
      },
    );

    const importButton = await $('[data-slot="profile-import-button"]');
    const createButton = await importButton.$('button[aria-label="创建配置"]');
    await createButton.waitForClickable({ timeout: 15_000 });
    await createButton.click();

    const localButton = await importButton.$('button[aria-label="本地配置"]');
    await localButton.waitForClickable({ timeout: 15_000 });
    await localButton.click();

    const dialog = await $('[data-slot="base-dialog"]');
    await dialog.waitForDisplayed({ timeout: 15_000 });
    assert.equal(await dialog.$('[name="url"]').isExisting(), false);

    const nameInput = await dialog.$('[name="name"]');
    await nameInput.waitForDisplayed({ timeout: 15_000 });
    await nameInput.setValue('Discarded local profile draft');
    assert.equal(await nameInput.getValue(), 'Discarded local profile draft');

    const closeButton = await dialog.$('[data-slot="dialog-close-button"]');
    await closeButton.waitForClickable({ timeout: 15_000 });
    await closeButton.click();
    await browser.waitUntil(
      async () =>
        browser.execute(
          () => !document.querySelector('[data-slot="base-dialog"]'),
        ),
      {
        timeout: 15_000,
        timeoutMsg: 'The local profile draft dialog did not close.',
      },
    );

    const inspectEmptyState = () =>
      browser.execute(() => {
        const text = document.body?.innerText ?? '';
        return {
          profileCards: document.querySelectorAll('[data-slot="profile-card"]')
            .length,
          emptyStateVisible: text.includes(
            '没有找到任何配置，请尝试导入或创建配置。',
          ),
        };
      });

    assert.deepEqual(await inspectEmptyState(), {
      profileCards: 0,
      emptyStateVisible: true,
    });

    await browser.refresh();
    await browser.waitUntil(
      async () => {
        const state = await inspectEmptyState();
        return state.profileCards === 0 && state.emptyStateVisible;
      },
      {
        timeout: 30_000,
        timeoutMsg: 'The cancelled local profile draft persisted after reload.',
      },
    );
  });
});
