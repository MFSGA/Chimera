import assert from 'node:assert/strict';
import { ensureMainWindow, openMainRoute } from './main-window.js';

const settingsHub = '/main/settings';
const backSelector = '[data-slot="settings-title"] a[href="/main/settings"]';

async function checkBackButton(pathname: string, visible: boolean) {
  await openMainRoute(pathname);
  await browser.waitUntil(
    async () =>
      browser.execute(
        (route, shouldShowBack) => {
          const title = document.querySelector<HTMLElement>(
            '[data-slot="settings-title"]',
          );
          const back = title?.querySelector<HTMLAnchorElement>(
            'a[href="/main/settings"]',
          );
          const backVisible =
            back !== null &&
            back !== undefined &&
            back.getClientRects().length > 0;
          return (
            location.pathname === route &&
            title !== null &&
            backVisible === shouldShowBack
          );
        },
        pathname,
        visible,
      ),
    {
      timeout: 15_000,
      timeoutMsg: `Settings back button visibility did not settle: ${pathname}, expected ${visible}.`,
    },
  );

  const observed = await browser.execute(() => {
    const title = document.querySelector<HTMLElement>(
      '[data-slot="settings-title"]',
    );
    const back = title?.querySelector<HTMLAnchorElement>(
      'a[href="/main/settings"]',
    );
    return {
      path: location.pathname,
      backLabel: back?.getAttribute('aria-label'),
      backHref: back?.getAttribute('href'),
      viewportWidth: innerWidth,
    };
  });

  assert.equal(observed.path, pathname);
  if (visible) {
    assert.equal(observed.backHref, settingsHub);
    assert.ok(observed.backLabel?.trim(), 'Back link needs an accessible name');
  }
  return observed;
}

async function clickVisibleSettingsLink(href: string) {
  const matches = await $$(
    `[data-slot="settings-sidebar-scroll-area"] a[href="${href}"]`,
  );
  for (const match of matches) {
    if (await match.isDisplayed()) {
      await match.waitForClickable({ timeout: 15_000 });
      await match.click();
      return;
    }
  }
  throw new Error(`No visible settings navigation link: ${href}`);
}

describe('settings route crumbs and actual back navigation', () => {
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
      throw new AggregateError(
        cleanupErrors,
        'Settings navigation cleanup failed.',
      );
    }
  });

  it('keeps the sidebar hub on desktop without a redundant back button', async () => {
    const routes = [
      '/main/settings/clash/network',
      '/main/settings/system/service',
      '/main/settings/chimera/tray',
      '/main/settings/chimera',
    ];
    for (const route of routes) {
      const observed = await checkBackButton(route, false);
      assert.ok(
        observed.viewportWidth >= 900,
        `Desktop breakpoint was not reached: ${JSON.stringify(observed)}`,
      );
    }
  });

  it('returns from a direct child to the mobile settings hub via a real click', async () => {
    await browser.setWindowSize(460, 760);
    const route = '/main/settings/chimera/tray';
    const observed = await checkBackButton(route, true);
    assert.ok(
      observed.viewportWidth < 600,
      `Mobile breakpoint was not reached: ${JSON.stringify(observed)}`,
    );

    const back = await $(backSelector);
    await back.waitForClickable({ timeout: 15_000 });
    await back.click();

    await browser.waitUntil(
      async () =>
        browser.execute(
          (hub) =>
            location.pathname.replace(/\/$/, '') === hub &&
            Array.from(
              document.querySelectorAll<HTMLAnchorElement>(
                '[data-slot="settings-sidebar-scroll-area"] a',
              ),
            ).some(
              (link) =>
                link.getAttribute('href') === '/main/settings/clash/network' &&
                link.getClientRects().length > 0,
            ),
          settingsHub,
        ),
      {
        timeout: 15_000,
        timeoutMsg: 'The mobile back button did not reveal the settings hub.',
      },
    );

    await openMainRoute(settingsHub);
    await clickVisibleSettingsLink('/main/settings/clash/network');
    await checkBackButton('/main/settings/clash/network', true);
  });

  it('keeps the legacy composite settings entry usable on mobile', async () => {
    await checkBackButton('/main/settings/clash', true);
    const back = await $(backSelector);
    await back.waitForClickable({ timeout: 15_000 });
    await back.click();
    await browser.waitUntil(
      async () =>
        browser.execute(
          (hub) => location.pathname.replace(/\/$/, '') === hub,
          settingsHub,
        ),
      {
        timeout: 15_000,
        timeoutMsg: 'Legacy Clash settings did not return to settings hub.',
      },
    );
  });
});
