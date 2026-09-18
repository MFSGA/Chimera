export interface ClashInfo {
  secret?: string;
  server: string;
}

export async function readClashInfo(): Promise<ClashInfo> {
  return browser.execute(async () => {
    const tauri = (
      window as typeof window & {
        __TAURI_INTERNALS__: {
          invoke: (command: string) => Promise<ClashInfo>;
        };
      }
    ).__TAURI_INTERNALS__;
    return tauri.invoke('get_clash_info');
  });
}

export async function readClashRuntimeConfig<T>(): Promise<T> {
  const deadline = Date.now() + 30_000;
  let lastError: unknown;
  let lastUrl = '<unknown>';

  while (Date.now() < deadline) {
    try {
      const info = await readClashInfo();
      lastUrl = `http://${info.server}/configs`;
      const remaining = deadline - Date.now();
      if (remaining <= 0) {
        break;
      }

      const response = await fetch(lastUrl, {
        headers: info.secret
          ? { Authorization: `Bearer ${info.secret}` }
          : undefined,
        signal: AbortSignal.timeout(Math.min(5_000, remaining)),
      });
      if (!response.ok) {
        throw new Error(`Clash config query failed: ${response.status}`);
      }
      return (await response.json()) as T;
    } catch (error) {
      lastError = error;
      const remaining = deadline - Date.now();
      if (remaining > 0) {
        await new Promise((resolve) =>
          setTimeout(resolve, Math.min(500, remaining)),
        );
      }
    }
  }

  throw new Error(`Clash runtime was not ready at ${lastUrl}`, {
    cause: lastError,
  });
}
