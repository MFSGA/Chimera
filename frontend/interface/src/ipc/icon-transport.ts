import { convertFileSrc, isTauri } from '@tauri-apps/api/core';
import type { TrayIcon } from './bindings';

export type IconRequest =
  | { kind: 'cached'; url: string }
  | { kind: 'tray' | 'widget'; mode: TrayIcon; version?: number | null };

/** Builds image URLs without invoking application commands. */
export interface RpcIconTransport {
  url(request: IconRequest): string;
}

/** Query encoding follows the shared backend icon parser on both transports. */
export function createIconTransport(desktop: boolean): RpcIconTransport {
  return {
    url(request) {
      if (!desktop) return request.kind === 'cached' ? request.url : '';
      const query = new URLSearchParams();
      if (request.kind === 'cached') {
        query.set('url', request.url);
      } else {
        query.set('mode', request.mode);
        if (request.version != null) {
          query.set('version', String(request.version));
        }
      }

      // convertFileSrc encodes its input as a path; append the query afterwards.
      const base = convertFileSrc(request.kind, 'chimera-icons');

      return `${base}?${query.toString()}`;
    },
  };
}

// The browser bridge is not yet present in Chimera.
export function cachedIconUrl(url: string): string {
  return createIconTransport(isTauri()).url({ kind: 'cached', url });
}
export function trayIconUrl(mode: TrayIcon, version?: number): string {
  return createIconTransport(isTauri()).url({ kind: 'tray', mode, version });
}
export function widgetIconUrl(mode: TrayIcon, version?: number): string {
  return createIconTransport(isTauri()).url({ kind: 'widget', mode, version });
}
