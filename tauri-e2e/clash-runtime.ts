interface ClashInfo {
  secret?: string;
  server: string;
}

export async function readClashRuntimeConfig<T>(): Promise<T> {
  const info = await browser.execute(async () => {
    const tauri = (
      window as typeof window & {
        __TAURI_INTERNALS__: {
          invoke: (command: string) => Promise<ClashInfo>;
        };
      }
    ).__TAURI_INTERNALS__;
    return tauri.invoke('get_clash_info');
  });
  const url = `http://${info.server}/configs`;
  const deadline = Date.now() + 30_000;
  let lastError: unknown;

  while (Date.now() < deadline) {
    try {
      const response = await fetch(url, {
        headers: info.secret
          ? { Authorization: `Bearer ${info.secret}` }
          : undefined,
      });
      if (!response.ok) {
        throw new Error(`Clash config query failed: ${response.status}`);
      }
      return (await response.json()) as T;
    } catch (error) {
      lastError = error;
      await new Promise((resolve) => setTimeout(resolve, 500));
    }
  }

  throw new Error(`Clash runtime was not ready at ${url}`, {
    cause: lastError,
  });
}
