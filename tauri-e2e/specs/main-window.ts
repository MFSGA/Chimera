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

export async function waitForApp(timeout = 30_000) {
  await browser.waitUntil(
    async () =>
      browser.execute(
        () => (document.getElementById('root')?.childElementCount ?? 0) > 0,
      ),
    { timeout, timeoutMsg: 'The Chimera frontend did not render.' },
  );
}

export async function ensureMainWindow() {
  if (!(await browser.getWindowHandles()).includes('main')) {
    await invoke('create_main_window');
    await browser.waitUntil(
      async () => (await browser.getWindowHandles()).includes('main'),
      { timeout: 15_000, timeoutMsg: 'The main window was not created.' },
    );
  }

  await browser.switchToWindow('main');
  await waitForApp();
}

export async function openMainRoute(pathname: string) {
  await ensureMainWindow();

  const currentHref = await browser.getUrl();
  if (new URL(currentHref).pathname !== pathname) {
    await browser.url(new URL(pathname, currentHref).href);
  }

  await waitForApp();
  await browser.waitUntil(
    async () =>
      browser.execute((expected) => location.pathname === expected, pathname),
    { timeout: 30_000, timeoutMsg: `${pathname} did not render.` },
  );
}
