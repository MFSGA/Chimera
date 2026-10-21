import assert from 'node:assert/strict';
import fs from 'node:fs';
import path from 'node:path';

const mainSettingsPath = '/main/settings/user-interface';

type AppConfig = {
  language?: string | null;
};

const supportedLocales = new Set(['en', 'ja', 'ru', 'zh-cn', 'zh-tw']);
const localeAliases: Record<string, string> = {
  'en-us': 'en',
  'ja-jp': 'ja',
};

function normalizeLocale(value: unknown): string | null {
  if (typeof value !== 'string') return null;

  const normalized = value.toLowerCase();
  const locale = localeAliases[normalized] ?? normalized;
  return supportedLocales.has(locale) ? locale : null;
}

type LocaleSnapshot = {
  cache: string | null;
  documentLanguage: string;
};

type WindowSize = {
  width: number;
  height: number;
};

function persistedLanguage(): string | null {
  const runtime = process.env.CHIMERA_E2E_RUNTIME_DIR;
  assert.ok(
    runtime,
    'CHIMERA_E2E_RUNTIME_DIR must be set by the WDIO harness.',
  );
  const configPath = path.join(runtime, 'config', 'application.yaml');
  if (!fs.existsSync(configPath)) return null;
  const text = fs.readFileSync(configPath, 'utf8');
  const value =
    /^language:\s*["']?([^"'#\s]+)["']?\s*$/m.exec(text)?.[1] ?? null;
  return value === 'null' || value === '~' ? null : value;
}

async function invoke<T>(command: string, args?: Record<string, unknown>) {
  return browser.execute(
    async (name, parameters) => {
      const internals = (
        window as typeof window & {
          __TAURI_INTERNALS__: {
            invoke: <R>(
              command: string,
              args?: Record<string, unknown>,
            ) => Promise<R>;
          };
        }
      ).__TAURI_INTERNALS__;
      return internals.invoke<T>(name, parameters);
    },
    command,
    args,
  );
}

async function openMainWindow() {
  await invoke('create_main_window');
  await browser.waitUntil(
    async () => (await browser.getWindowHandles()).includes('main'),
    { timeout: 15_000, timeoutMsg: 'The main window was not created.' },
  );
  await browser.switchToWindow('main');
}

async function readLocaleSnapshot(): Promise<LocaleSnapshot> {
  return browser.execute(() => ({
    cache: localStorage.getItem(btoa('paraglide-language-cache')),
    documentLanguage: document.documentElement.lang,
  }));
}

async function setLocaleCache(locale: string): Promise<void> {
  await browser.execute((value) => {
    localStorage.setItem(btoa('paraglide-language-cache'), value);
  }, locale);
}

async function openUserInterfaceSettings() {
  await browser.waitUntil(
    async () =>
      browser.execute(
        () =>
          document.documentElement.classList.contains('chimera-main') &&
          (document.getElementById('root')?.childElementCount ?? 0) > 0 &&
          document.querySelector('[data-slot="app-root"]') !== null,
      ),
    {
      timeout: 30_000,
      timeoutMsg: 'The main application did not finish rendering.',
    },
  );

  if ((await browser.execute(() => location.pathname)) !== mainSettingsPath) {
    await browser.execute((target) => {
      const link = Array.from(
        document.querySelectorAll<HTMLAnchorElement>('a'),
      ).find((candidate) => candidate.getAttribute('href') === target);

      if (link) {
        link.click();
        return;
      }

      history.pushState({}, '', target);
      window.dispatchEvent(new PopStateEvent('popstate'));
    }, mainSettingsPath);
  }

  await $('[data-slot="language-selector-card"]').waitForDisplayed({
    timeout: 15_000,
  });
}

async function waitForLocaleSynchronization(handle: string) {
  let lastState: unknown;
  try {
    await browser.waitUntil(
      async () => {
        const config = await invoke<AppConfig>('get_app_config');
        const ui = await browser.execute(() => ({
          ready:
            document.readyState === 'complete' &&
            (document.getElementById('root')?.childElementCount ?? 0) > 0,
          language: document.documentElement.lang.toLowerCase(),
          cachedLanguage: localStorage.getItem(
            btoa('paraglide-language-cache'),
          ),
        }));
        const configuredLocale = normalizeLocale(config.language);
        const cachedLocale = normalizeLocale(ui.cachedLanguage);
        const expectedLocale = configuredLocale ?? cachedLocale ?? 'en';
        const cacheSynchronized =
          configuredLocale === null || cachedLocale === configuredLocale;
        lastState = {
          handle,
          configured: config.language,
          cachedLocale,
          cacheSynchronized,
          expectedLocale,
          ...ui,
        };
        return ui.ready && ui.language === expectedLocale && cacheSynchronized;
      },
      {
        timeout: 30_000,
        interval: 250,
        timeoutMsg: `${handle} locale did not synchronize before its snapshot.`,
      },
    );
  } catch (cause) {
    throw new Error(
      `${handle} locale did not synchronize before its snapshot: ${JSON.stringify(lastState)}`,
      { cause },
    );
  }
}

async function waitForPreparedLocale(handle: string, locale: string) {
  let lastState: unknown;
  try {
    await browser.waitUntil(
      async () => {
        const ui = await browser.execute(() => ({
          ready:
            document.readyState === 'complete' &&
            (document.getElementById('root')?.childElementCount ?? 0) > 0,
          language: document.documentElement.lang.toLowerCase(),
          cachedLanguage: localStorage.getItem(
            btoa('paraglide-language-cache'),
          ),
        }));
        lastState = { handle, expected: locale, ...ui };
        return (
          ui.ready &&
          normalizeLocale(ui.language) === locale &&
          normalizeLocale(ui.cachedLanguage) === locale
        );
      },
      {
        timeout: 30_000,
        interval: 250,
        timeoutMsg: `${handle} did not render the prepared ${locale} locale.`,
      },
    );
  } catch (cause) {
    throw new Error(
      `${handle} did not render the prepared locale: ${JSON.stringify(lastState)}`,
      { cause },
    );
  }
}

async function restoreLocaleSnapshot(handle: string, snapshot: LocaleSnapshot) {
  if (!(await browser.getWindowHandles()).includes(handle)) return;

  await browser.switchToWindow(handle);
  await browser.execute((cache) => {
    const key = btoa('paraglide-language-cache');
    if (cache === null) localStorage.removeItem(key);
    else localStorage.setItem(key, cache);
  }, snapshot.cache);
  await browser.refresh();
  await browser.waitUntil(
    async () =>
      browser.execute(
        (language) => document.documentElement.lang === language,
        snapshot.documentLanguage,
      ),
    {
      timeout: 15_000,
      timeoutMsg: `${handle} did not return to its original language.`,
    },
  );
  await browser.execute((cache) => {
    const key = btoa('paraglide-language-cache');
    if (cache === null) localStorage.removeItem(key);
    else localStorage.setItem(key, cache);
  }, snapshot.cache);
}

async function restoreWindowState(
  handle: string,
  label: string,
  cleanup: () => Promise<void>,
  errors: Error[],
) {
  try {
    if (!(await browser.getWindowHandles()).includes(handle)) return;
    await browser.switchToWindow(handle);
    await cleanup();
  } catch (cause) {
    errors.push(new Error(`Could not restore ${handle} ${label}.`, { cause }));
  }
}

/**
 * Contract: Starting from the persisted English test setting, select Japanese
 * through the main UI keyboard menu. Verify immediate main UI/cache state,
 * typed app config, application.yaml, the main UI after refresh, and Japanese
 * labels after navigating the retained legacy window to /proxies. This catches
 * locale changes delayed behind query refetch; it does not claim live sync in
 * an already-open legacy page or verify a cold application restart. Restore
 * each synchronized locale snapshot, route, and window size afterward.
 */
describe('application language selection', () => {
  let originalLanguage: string | null | undefined;
  const localeSnapshots = new Map<string, LocaleSnapshot>();
  const originalUrls = new Map<string, string>();
  const originalWindowSizes = new Map<string, WindowSize>();

  before(async () => {
    await browser.waitUntil(
      async () =>
        browser.execute(
          () =>
            document.readyState === 'complete' &&
            (document.getElementById('root')?.childElementCount ?? 0) > 0,
        ),
      { timeout: 30_000, timeoutMsg: 'The Chimera frontend did not render.' },
    );

    const config = await invoke<AppConfig>('get_app_config');
    originalLanguage = config.language ?? null;
    for (const handle of await browser.getWindowHandles()) {
      if (handle !== 'legacy' && handle !== 'main') continue;
      await browser.switchToWindow(handle);
      originalWindowSizes.set(handle, await browser.getWindowSize());
      originalUrls.set(handle, await browser.getUrl());
      // Capture each WebView's actual prior state before normalizing its
      // Paraglide cache for this test's controlled starting locale.
      localeSnapshots.set(handle, await readLocaleSnapshot());
    }

    await openMainWindow();
    if (!localeSnapshots.has('main')) {
      // Register the newly created window before waiting for its first render so
      // after() can still restore it if startup times out.
      originalWindowSizes.set('main', await browser.getWindowSize());
      originalUrls.set('main', await browser.getUrl());
      await browser.waitUntil(
        async () =>
          browser.execute(
            () =>
              document.readyState === 'complete' &&
              (document.getElementById('root')?.childElementCount ?? 0) > 0,
          ),
        {
          timeout: 30_000,
          timeoutMsg: 'The new main window did not finish its first render.',
        },
      );
      localeSnapshots.set('main', await readLocaleSnapshot());
    }
    await browser.setWindowSize(1240, 638);
    await openUserInterfaceSettings();

    await invoke('patch_app_config', { patch: { language: 'en' } });
    await browser.waitUntil(
      async () =>
        ['en', 'en-us'].includes(
          (
            (await invoke<AppConfig>('get_app_config')).language ?? ''
          ).toLowerCase(),
        ),
      {
        timeout: 15_000,
        timeoutMsg: 'Could not prepare the test runtime in English.',
      },
    );
    // Earlier layout specs intentionally seed their own locales. Prepare both
    // retained WebViews from the same persisted English setting so this test
    // does not depend on which spec ran before it.
    for (const handle of localeSnapshots.keys()) {
      await browser.switchToWindow(handle);
      await setLocaleCache('en');
      await browser.refresh();
      await waitForPreparedLocale(handle, 'en');
    }
    await browser.switchToWindow('main');
    await openUserInterfaceSettings();
    await browser.waitUntil(
      async () =>
        browser.execute(
          () =>
            document.documentElement.lang === 'en' &&
            /English/.test(
              document.querySelector('[data-slot="language-selector-trigger"]')
                ?.textContent ?? '',
            ),
        ),
      {
        timeout: 15_000,
        timeoutMsg: 'The main UI did not render the prepared English locale.',
      },
    );
  });

  after(async () => {
    const cleanupErrors: Error[] = [];
    try {
      if (originalLanguage !== undefined) {
        await invoke('patch_app_config', {
          patch: { language: originalLanguage },
        });
        await browser.waitUntil(
          async () =>
            ((await invoke<AppConfig>('get_app_config')).language ?? null) ===
            originalLanguage,
          {
            timeout: 15_000,
            timeoutMsg: 'The original language setting was not restored.',
          },
        );
        await browser.waitUntil(
          () => persistedLanguage() === originalLanguage,
          {
            timeout: 15_000,
            timeoutMsg:
              'application.yaml did not restore the original language setting.',
          },
        );
      }
    } catch (cause) {
      cleanupErrors.push(
        new Error('Could not restore the original language setting.', {
          cause,
        }),
      );
    }

    for (const handle of originalUrls.keys()) {
      const snapshot = localeSnapshots.get(handle);
      if (snapshot) {
        await restoreWindowState(
          handle,
          'locale',
          () => restoreLocaleSnapshot(handle, snapshot),
          cleanupErrors,
        );
      } else {
        // A setup failure can happen after refresh but before synchronization.
        // Re-resolve from the restored persisted config rather than retaining a
        // partially initialized runtime locale.
        await restoreWindowState(
          handle,
          'locale synchronization fallback',
          async () => {
            await browser.refresh();
            await waitForLocaleSynchronization(handle);
          },
          cleanupErrors,
        );
      }
      await restoreWindowState(
        handle,
        'route',
        async () => {
          const originalUrl = originalUrls.get(handle);
          if (originalUrl && (await browser.getUrl()) !== originalUrl) {
            await browser.url(originalUrl);
            await browser.waitUntil(
              async () => (await browser.getUrl()) === originalUrl,
              {
                timeout: 15_000,
                timeoutMsg: `${handle} did not return to its original route.`,
              },
            );
          }
        },
        cleanupErrors,
      );
      await restoreWindowState(
        handle,
        'dimensions',
        async () => {
          const originalSize = originalWindowSizes.get(handle);
          if (originalSize) {
            await browser.setWindowSize(
              originalSize.width,
              originalSize.height,
            );
          }
        },
        cleanupErrors,
      );
    }

    try {
      if ((await browser.getWindowHandles()).includes('legacy')) {
        await browser.switchToWindow('legacy');
      }
    } catch (cause) {
      cleanupErrors.push(
        new Error('Could not return focus to the legacy window.', { cause }),
      );
    }
    if (cleanupErrors.length > 0) {
      throw new AggregateError(
        cleanupErrors,
        `Application language cleanup was incomplete: ${cleanupErrors
          .map((error) => error.message)
          .join('; ')}`,
      );
    }
  });

  it('loads the persisted Japanese locale in the legacy proxy page', async () => {
    const trigger = await $('[data-slot="language-selector-trigger"]');
    await trigger.waitForDisplayed();
    await browser.execute((element) => element.focus(), trigger);
    await browser.keys('Enter');

    await browser.waitUntil(
      async () => (await trigger.getAttribute('aria-expanded')) === 'true',
      {
        timeout: 10_000,
        timeoutMsg: 'The language menu did not open from the keyboard.',
      },
    );
    const menu = await $('[data-slot="language-selector-menu"]');
    await menu.waitForExist({ timeout: 10_000 });
    const japaneseOption = await $(
      '//*[@data-slot="language-selector-menu"]//*[@role="menuitemcheckbox" and normalize-space(.)="日本語"]',
    );
    await japaneseOption.waitForExist({ timeout: 10_000 });
    await browser.keys('ArrowDown');
    await browser.waitUntil(
      async () =>
        browser.execute(
          () => document.activeElement?.textContent?.trim() === '日本語',
        ),
      {
        timeout: 10_000,
        timeoutMsg: 'ArrowDown did not focus the Japanese language option.',
      },
    );
    await browser.keys('Enter');

    let lastLanguageState: unknown;
    try {
      await browser.waitUntil(
        async () => {
          const ui = await browser.execute(() => ({
            language: document.documentElement.lang,
            cachedLanguage: localStorage.getItem(
              btoa('paraglide-language-cache'),
            ),
            readyState: document.readyState,
            selectedLanguage: document
              .querySelector('[data-slot="language-selector-trigger"]')
              ?.textContent?.trim(),
          }));
          const japaneseIsSynchronized =
            ui.language.toLowerCase() === 'ja' &&
            normalizeLocale(ui.cachedLanguage) === 'ja' &&
            /日本語/.test(ui.selectedLanguage ?? '');
          lastLanguageState = { ...ui, japaneseIsSynchronized };
          // Persistence, reload, and legacy-window assertions below prove the
          // durable behavior. Requiring repeated remote WebDriver snapshots
          // can time out after the UI has already reached the requested locale.
          return japaneseIsSynchronized;
        },
        {
          timeout: 30_000,
          interval: 500,
          timeoutMsg: 'Japanese was not applied to the active UI.',
        },
      );
    } catch (cause) {
      throw new Error(
        `Japanese was not applied to the active UI: ${JSON.stringify(lastLanguageState)}`,
        { cause },
      );
    }
    const config = await invoke<AppConfig>('get_app_config');
    assert.equal(config.language?.toLowerCase(), 'ja');
    await browser.waitUntil(() => persistedLanguage() === 'ja', {
      timeout: 15_000,
      timeoutMsg: 'application.yaml did not persist the Japanese language.',
    });

    const currentMainState = await browser.execute(() => ({
      language: document.documentElement.lang,
      selectedLanguage: document
        .querySelector('[data-slot="language-selector-trigger"]')
        ?.textContent?.trim(),
    }));
    assert.equal(currentMainState.language, 'ja');
    assert.match(currentMainState.selectedLanguage ?? '', /日本語/);

    await browser.refresh();
    await openUserInterfaceSettings();

    const mainState = await browser.execute(() => ({
      language: document.documentElement.lang,
      selectedLanguage: document
        .querySelector('[data-slot="language-selector-trigger"]')
        ?.textContent?.trim(),
    }));
    assert.equal(mainState.language, 'ja');
    assert.match(mainState.selectedLanguage ?? '', /日本語/);

    await browser.switchToWindow('legacy');
    const legacyUrl = new URL(await browser.getUrl());
    legacyUrl.pathname = '/proxies';
    legacyUrl.search = '';
    await browser.url(legacyUrl.href);
    let lastLegacyState:
      | {
          pathname: string;
          readyState: string;
          rootMounted: boolean;
          language: string;
          cachedLanguage: string | null;
          title: string | null;
          modeLabels: Array<[string, string]>;
        }
      | undefined;
    try {
      await browser.waitUntil(
        async () => {
          const state = await browser.execute(() => {
            const title = document.querySelector('h1')?.textContent?.trim();
            const modeLabels = Array.from(
              document.querySelectorAll<HTMLButtonElement>(
                '[role="group"] button',
              ),
            ).map(
              (button) =>
                [button.value, button.textContent?.trim() ?? ''] as [
                  string,
                  string,
                ],
            );
            return {
              pathname: location.pathname,
              readyState: document.readyState,
              rootMounted:
                (document.getElementById('root')?.childElementCount ?? 0) > 0,
              language: document.documentElement.lang,
              cachedLanguage: localStorage.getItem(
                btoa('paraglide-language-cache'),
              ),
              title: title ?? null,
              modeLabels,
            };
          });
          lastLegacyState = state;
          return (
            state.pathname === '/proxies' &&
            state.readyState === 'complete' &&
            state.rootMounted &&
            state.language === 'ja' &&
            normalizeLocale(state.cachedLanguage) === 'ja' &&
            state.title === 'プロキシ' &&
            state.modeLabels.some(
              ([value, label]) => value === 'rule' && label === 'ルール',
            ) &&
            state.modeLabels.some(
              ([value, label]) => value === 'global' && label === 'グローバル',
            ) &&
            state.modeLabels.some(
              ([value, label]) => value === 'direct' && label === 'ダイレクト',
            )
          );
        },
        {
          timeout: 30_000,
          timeoutMsg:
            'The legacy proxy page did not render its Japanese labels after navigation.',
        },
      );
    } catch (cause) {
      throw new Error(
        `The legacy proxy page did not synchronize to Japanese: ${JSON.stringify(lastLegacyState)}`,
        { cause },
      );
    }

    assert.ok(lastLegacyState);
    assert.deepEqual(
      {
        pathname: lastLegacyState.pathname,
        language: lastLegacyState.language,
        cachedLanguage: normalizeLocale(lastLegacyState.cachedLanguage),
        title: lastLegacyState.title,
      },
      {
        pathname: '/proxies',
        language: 'ja',
        cachedLanguage: 'ja',
        title: 'プロキシ',
      },
    );
  });
});
