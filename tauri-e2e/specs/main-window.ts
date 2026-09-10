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

async function waitForMainApp(timeout = 30_000) {
  await browser.waitUntil(
    async () =>
      browser.execute(() => {
        const root = document.getElementById('root');
        return (
          (root?.childElementCount ?? 0) > 0 &&
          document.documentElement.classList.contains('chimera-main') &&
          document.querySelector('[data-slot="app-header"]') !== null
        );
      }),
    { timeout, timeoutMsg: 'The Chimera main frontend did not render.' },
  );
}

async function createMainWindow() {
  const handles = await browser.getWindowHandles();
  if (!handles.includes('legacy')) {
    throw new Error('The legacy E2E window is unavailable.');
  }

  await browser.switchToWindow('legacy');
  await invoke('create_main_window');
  await browser.waitUntil(
    async () => (await browser.getWindowHandles()).includes('main'),
    { timeout: 15_000, timeoutMsg: 'The main window was not created.' },
  );
  await browser.switchToWindow('main');
  await waitForMainApp();
}

const animatedOutletHosts = [
  '[data-slot="app-content"]',
  '[data-slot="settings-content"]',
  '[data-slot="providers-content"]',
  '[data-slot="profiles-content"]',
  '[data-slot="proxies-content"]',
] as const;

async function waitForMainRouteSettled(timeout = 15_000) {
  await browser.waitUntil(
    async () =>
      browser.execute((selectors) => {
        const hostGroups = selectors.map((selector) =>
          Array.from(document.querySelectorAll<HTMLElement>(selector)),
        );
        const appContent = hostGroups[0];
        if (appContent.length !== 1 || appContent[0].children.length === 0) {
          return false;
        }

        return hostGroups.slice(1).every((hosts) => {
          if (hosts.length > 1) return false;
          return hosts.length === 0 || hosts[0].children.length === 1;
        });
      }, animatedOutletHosts),
    {
      timeout,
      timeoutMsg: 'The Chimera main route animation did not settle.',
    },
  );
}

export async function ensureMainWindow() {
  if (!(await browser.getWindowHandles()).includes('main')) {
    await createMainWindow();
    return;
  }

  await browser.switchToWindow('main');
  await waitForMainApp();
}

export async function openMainRoute(pathname: string) {
  await ensureMainWindow();

  if ((await browser.execute(() => location.pathname)) !== pathname) {
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
    }, pathname);
  }

  await browser.waitUntil(
    async () =>
      browser.execute(
        (expected) =>
          location.pathname === expected &&
          document.documentElement.classList.contains('chimera-main') &&
          document.querySelector('[data-slot="app-header"]') !== null,
        pathname,
      ),
    { timeout: 30_000, timeoutMsg: `${pathname} did not render.` },
  );
  await waitForMainRouteSettled();
}
