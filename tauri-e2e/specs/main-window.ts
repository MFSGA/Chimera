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

async function recoverStaleMainWindow() {
  if ((await browser.getWindowHandles()).includes('main')) {
    await browser.switchToWindow('main');
    await browser.closeWindow();
    await browser.waitUntil(
      async () => !(await browser.getWindowHandles()).includes('main'),
      { timeout: 15_000, timeoutMsg: 'The stale main window was not closed.' },
    );
  }

  await createMainWindow();
}

export async function ensureMainWindow() {
  if (!(await browser.getWindowHandles()).includes('main')) {
    await createMainWindow();
    return;
  }

  await browser.switchToWindow('main');
  try {
    await waitForMainApp(5_000);
  } catch {
    await recoverStaleMainWindow();
  }
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
}
