import assert from 'node:assert/strict';
import fs from 'node:fs';
import path from 'node:path';
import { ensureMainWindow, openMainRoute } from './main-window.js';

const targetPath = '/main/settings/system/proxy';
type ThemeMode = 'light' | 'dark' | 'system';

let originalThemeMode: ThemeMode = 'system';
let themeModeOverrideInstalled = false;

async function readThemeMode(): Promise<ThemeMode> {
  return browser.execute(async () => {
    const internals = (
      window as Window & {
        __TAURI_INTERNALS__?: {
          invoke: <T>(command: string) => Promise<T>;
        };
      }
    ).__TAURI_INTERNALS__;
    if (!internals) throw new Error('Tauri page internals are unavailable.');
    const config = await internals.invoke<{ theme_mode?: ThemeMode }>(
      'get_verge_config',
    );
    return config.theme_mode ?? 'system';
  });
}

async function writeThemeMode(themeMode: ThemeMode) {
  await browser.execute(async (mode) => {
    const internals = (
      window as Window & {
        __TAURI_INTERNALS__?: {
          invoke: <T>(
            command: string,
            args?: Record<string, unknown>,
          ) => Promise<T>;
        };
      }
    ).__TAURI_INTERNALS__;
    if (!internals) throw new Error('Tauri page internals are unavailable.');
    await internals.invoke<null>('patch_verge_config', {
      payload: { theme_mode: mode },
    });
  }, themeMode);
}

async function clickVisibleSettingsLink(href: string) {
  for (const link of await $$(
    `[data-slot="settings-sidebar-scroll-area"] a[href="${href}"]`,
  )) {
    if (await link.isDisplayed()) {
      await link.click();
      return;
    }
  }
  throw new Error(`No visible settings link: ${href}`);
}

async function waitForActiveSettingsLink(href: string) {
  try {
    await browser.waitUntil(
      async () =>
        browser.execute((target) => {
          const link = Array.from(
            document.querySelectorAll<HTMLElement>(
              '[data-slot="settings-sidebar-scroll-area"] a',
            ),
          ).find(
            (candidate) =>
              candidate.getAttribute('href') === target &&
              candidate.getClientRects().length > 0 &&
              candidate.getAttribute('data-active') === 'true' &&
              candidate.getAttribute('aria-current') === 'page',
          );
          return location.pathname === target && link !== undefined;
        }, href),
      { timeout: 15_000 },
    );
  } catch (error) {
    const state = await browser.execute(() => ({
      pathname: location.pathname,
      href: location.href,
      links: Array.from(
        document.querySelectorAll<HTMLElement>(
          '[data-slot="settings-sidebar-scroll-area"] a',
        ),
      )
        .filter((link) => link.getClientRects().length > 0)
        .map((link) => ({
          href: link.getAttribute('href'),
          active: link.getAttribute('data-active'),
          current: link.getAttribute('aria-current'),
        })),
    }));
    throw new Error(
      `The visible settings link did not become active: ${href}; state=${JSON.stringify(state)}`,
      { cause: error },
    );
  }
}

async function visibleElement(selector: string) {
  let visible: WebdriverIO.Element | undefined;
  await browser.waitUntil(
    async () => {
      for (const element of await $$(selector)) {
        if (await element.isDisplayed()) {
          visible = element;
          return true;
        }
      }
      return false;
    },
    {
      timeout: 15_000,
      timeoutMsg: `No visible element matches: ${selector}`,
    },
  );
  return visible!;
}

describe('main settings reference layout', () => {
  let originalWindowSize: { width: number; height: number } | undefined;
  let originalLanguage: string | null | undefined;

  before(async () => {
    await ensureMainWindow();
    originalWindowSize = await browser.getWindowSize();
    originalLanguage = await browser.execute(() =>
      localStorage.getItem(btoa('paraglide-language-cache')),
    );
    await browser.setWindowSize(1240, 638);
    await browser.execute(() => {
      localStorage.setItem(btoa('paraglide-language-cache'), 'zh-cn');
    });
    await openMainRoute(targetPath);
    originalThemeMode = await readThemeMode();
    await writeThemeMode('light');
    themeModeOverrideInstalled = true;
    await browser.waitUntil(
      async () =>
        browser.execute(() =>
          document.documentElement.classList.contains('light'),
        ),
      {
        timeout: 15_000,
        timeoutMsg: 'The settings layout did not enter light theme.',
      },
    );
    await browser.execute(async () => {
      await document.fonts.ready;
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
    if (themeModeOverrideInstalled) {
      await attemptCleanup('restore the original theme', async () => {
        await writeThemeMode(originalThemeMode);
        await browser.waitUntil(
          async () =>
            browser.execute((mode) => {
              const theme =
                mode === 'system'
                  ? window.matchMedia('(prefers-color-scheme: dark)').matches
                    ? 'dark'
                    : 'light'
                  : mode;
              return document.documentElement.classList.contains(theme);
            }, originalThemeMode),
          {
            timeout: 15_000,
            timeoutMsg: 'The original settings theme was not restored.',
          },
        );
      });
    }
    const language = originalLanguage;
    if (language !== undefined) {
      await attemptCleanup('restore the original language', async () => {
        await browser.execute((language) => {
          const key = btoa('paraglide-language-cache');
          if (language === null) localStorage.removeItem(key);
          else localStorage.setItem(key, language);
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
        'Settings layout cleanup failed.',
      );
    }
  });

  it('renders grouped reference navigation and retains the full-height content layout', async () => {
    await waitForActiveSettingsLink(targetPath);

    const state = await browser.execute(() => {
      const proxyLink =
        Array.from(
          document.querySelectorAll<HTMLElement>(
            '[data-slot="settings-sidebar-scroll-area"] a[href="/main/settings/system/proxy"]',
          ),
        ).find(
          (link) =>
            link.getClientRects().length > 0 &&
            link.getAttribute('data-active') === 'true' &&
            link.getAttribute('aria-current') === 'page',
        ) ?? null;
      const container =
        proxyLink?.closest<HTMLElement>('[data-slot="settings-container"]') ??
        null;
      const sidebar =
        container?.querySelector<HTMLElement>(
          '[data-slot="settings-sidebar-scroll-area"]',
        ) ?? null;
      const content =
        container?.querySelector<HTMLElement>(
          '[data-slot="settings-content"]',
        ) ?? null;
      const animatedOutlet = content?.firstElementChild as HTMLElement | null;
      const routeContent =
        animatedOutlet?.firstElementChild as HTMLElement | null;
      const nav = sidebar?.querySelector<HTMLElement>(
        '[data-slot="navigation-list"]',
      );
      const rect = (element: HTMLElement | null) => {
        if (!element) return null;
        const bounds = element.getBoundingClientRect();
        return {
          x: Math.round(bounds.x),
          y: Math.round(bounds.y),
          width: Math.round(bounds.width),
          height: Math.round(bounds.height),
        };
      };

      return {
        viewport: { width: innerWidth, height: innerHeight },
        container: rect(container),
        sidebar: rect(sidebar),
        content: rect(content),
        animatedOutlet: rect(animatedOutlet),
        routeContent: rect(routeContent),
        outletDisplay:
          animatedOutlet && getComputedStyle(animatedOutlet).display,
        outletDirection:
          animatedOutlet && getComputedStyle(animatedOutlet).flexDirection,
        outletGrow: animatedOutlet && getComputedStyle(animatedOutlet).flexGrow,
        navigation: rect(nav ?? null),
        navigationGroups:
          nav?.querySelectorAll('[data-slot="navigation-list-group"]').length ??
          0,
        navigationItems:
          nav?.querySelectorAll('[data-slot="navigation-list-item"]').length ??
          0,
        proxyLink: rect(proxyLink ?? null),
        proxyActive: proxyLink?.getAttribute('data-active'),
        proxyCurrent: proxyLink?.getAttribute('aria-current'),
      };
    });

    assert.ok(state.viewport.width >= 1200, JSON.stringify(state));
    assert.ok(state.viewport.height >= 600, JSON.stringify(state));
    assert.ok(
      state.container && state.sidebar && state.content,
      JSON.stringify(state),
    );
    assert.ok(state.navigation && state.proxyLink, JSON.stringify(state));
    assert.equal(state.navigationGroups, 4, JSON.stringify(state));
    assert.equal(state.navigationItems, 16, JSON.stringify(state));
    assert.equal(state.proxyActive, 'true', JSON.stringify(state));
    assert.equal(state.proxyCurrent, 'page', JSON.stringify(state));
    assert.equal(state.outletDisplay, 'flex', JSON.stringify(state));
    assert.equal(state.outletDirection, 'column', JSON.stringify(state));
    assert.equal(state.outletGrow, '1', JSON.stringify(state));
    assert.ok(
      (state.animatedOutlet?.height ?? 0) >= (state.container?.height ?? 0) - 2,
      JSON.stringify(state),
    );
    assert.ok(state.routeContent, JSON.stringify(state));

    const evidencePath = process.env.CHIMERA_E2E_EVIDENCE_PATH;
    if (evidencePath) {
      fs.mkdirSync(path.dirname(evidencePath), { recursive: true });
      await browser.saveScreenshot(evidencePath);
    }
  });

  it('navigates from proxy settings to service settings and back through visible links', async () => {
    await clickVisibleSettingsLink('/main/settings/system/service');

    const startup = await visibleElement(
      '[data-slot="system-launch-container"]',
    );
    await startup.waitForDisplayed({ timeout: 15_000 });
    await waitForActiveSettingsLink('/main/settings/system/service');

    await clickVisibleSettingsLink('/main/settings/system/proxy');

    const mode = await visibleElement('[data-slot="proxy-mode-container"]');
    await mode.waitForDisplayed({ timeout: 15_000 });
    await waitForActiveSettingsLink('/main/settings/system/proxy');
  });

  it('opens the reference control-channel settings through the sidebar', async () => {
    await clickVisibleSettingsLink('/main/settings/clash/control');

    await waitForActiveSettingsLink('/main/settings/clash/control');

    assert.equal(
      new URL(await browser.getUrl()).pathname,
      '/main/settings/clash/control',
    );
  });

  it('opens network settings and displays the existing port controls', async () => {
    await clickVisibleSettingsLink('/main/settings/clash/network');

    const portSettings = await visibleElement(
      '[data-slot="port-settings-container"]',
    );
    await portSettings.waitForDisplayed({ timeout: 15_000 });

    await waitForActiveSettingsLink('/main/settings/clash/network');

    assert.equal(
      new URL(await browser.getUrl()).pathname,
      '/main/settings/clash/network',
    );
  });

  it('opens the tray and window settings from the new reference navigation', async () => {
    await clickVisibleSettingsLink('/main/settings/chimera/tray');

    const traySettings = await visibleElement(
      '[data-slot="tray-settings-container"]',
    );
    const windowSettings = await visibleElement(
      '[data-slot="window-settings-container"]',
    );
    await traySettings.waitForDisplayed({ timeout: 15_000 });
    await windowSettings.waitForDisplayed({ timeout: 15_000 });

    await waitForActiveSettingsLink('/main/settings/chimera/tray');
    assert.equal(
      new URL(await browser.getUrl()).pathname,
      '/main/settings/chimera/tray',
    );
  });

  it('opens public IP settings while retaining the original controls', async () => {
    await clickVisibleSettingsLink('/main/settings/chimera/public-ip');

    const settings = await visibleElement('[data-slot="public-ip-settings"]');
    await settings.waitForDisplayed({ timeout: 15_000 });

    await waitForActiveSettingsLink('/main/settings/chimera/public-ip');
    assert.equal(
      new URL(await browser.getUrl()).pathname,
      '/main/settings/chimera/public-ip',
    );
  });
});
