// Native transport boundary: only this shared IPC adapter constructs Channels.
// See the scoped exception in scripts/check-frontend-boundaries.ts.
import { Channel, isTauri } from '@tauri-apps/api/core';

export { isTauri };

export function createConnectionDetailsChannel<T>(receive: (frame: T) => void) {
  const channel = new Channel<T>();
  channel.onmessage = receive;
  return channel;
}
