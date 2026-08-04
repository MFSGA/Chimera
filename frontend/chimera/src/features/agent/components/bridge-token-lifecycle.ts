export const TOKEN_DISPLAY_LIFETIME_MS = 60_000;
export const TOKEN_CLIPBOARD_LIFETIME_MS = 30_000;

export type BridgeTokenEvent =
  | { type: 'started'; token: string | null }
  | { type: 'running_changed'; running: boolean }
  | { type: 'copied'; token: string }
  | { type: 'expired'; token: string };

export interface TimeoutScheduler {
  set(callback: () => void | Promise<void>, delayMs: number): unknown;
  clear(handle: unknown): void;
}

export interface ClipboardAccess {
  readText(): Promise<string>;
  writeText(value: string): Promise<void>;
}

const defaultScheduler: TimeoutScheduler = {
  set: (callback, delayMs) => globalThis.setTimeout(callback, delayMs),
  clear: (handle) => globalThis.clearTimeout(handle as number),
};

export function reduceBridgeToken(
  current: string | null,
  event: BridgeTokenEvent,
): string | null {
  switch (event.type) {
    case 'started':
      return event.token;
    case 'running_changed':
      return event.running ? current : null;
    case 'copied':
    case 'expired':
      return current === event.token ? null : current;
  }
}

export function scheduleTokenExpiry(
  token: string,
  expire: (token: string) => void,
  scheduler: TimeoutScheduler = defaultScheduler,
  delayMs = TOKEN_DISPLAY_LIFETIME_MS,
) {
  const handle = scheduler.set(() => expire(token), delayMs);
  return () => scheduler.clear(handle);
}

export function scheduleClipboardValueClear(
  value: string,
  clipboard: ClipboardAccess,
  scheduler: TimeoutScheduler = defaultScheduler,
  delayMs = TOKEN_CLIPBOARD_LIFETIME_MS,
) {
  scheduler.set(async () => {
    try {
      if ((await clipboard.readText()) === value) await clipboard.writeText('');
    } catch {
      // Cleanup is best effort and must never surface or redisplay the token.
    }
  }, delayMs);
}
