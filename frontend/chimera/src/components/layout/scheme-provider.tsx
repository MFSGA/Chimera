import { commands, unwrapResult } from '@chimera/interface';
import { useNavigate } from '@tanstack/react-router';
import { listen, type UnlistenFn } from '@tauri-apps/api/event';
import { getCurrentWebviewWindow } from '@tauri-apps/api/webviewWindow';
import { useEffect, useRef } from 'react';

const normalizeSchemePath = (url: URL) => {
  let pathname = `${url.hostname || ''}${url.pathname || ''}`;

  if (pathname.endsWith('/')) {
    pathname = pathname.slice(0, -1);
  }

  if (pathname.startsWith('//')) {
    pathname = pathname.slice(2);
  }

  return pathname;
};

const decodeSearchParam = (value: string | null) => {
  if (!value) {
    return undefined;
  }

  try {
    return decodeURIComponent(value);
  } catch {
    return value;
  }
};

const APP_WINDOW_LABELS = new Set(['legacy', 'main']);
const DUPLICATE_WINDOW_MS = 2_000;

export const SchemeProvider = () => {
  const navigate = useNavigate();
  const unlistenRef = useRef<UnlistenFn | null>(null);
  const lastHandledRef = useRef<{ raw: string; at: number } | null>(null);

  useEffect(() => {
    const windowLabel = getCurrentWebviewWindow().label;
    if (!APP_WINDOW_LABELS.has(windowLabel)) {
      return;
    }

    let disposed = false;

    const handleSchemeRequest = async (raw: string) => {
      const now = Date.now();
      const lastHandled = lastHandledRef.current;
      if (
        lastHandled?.raw === raw &&
        now - lastHandled.at < DUPLICATE_WINDOW_MS
      ) {
        return;
      }
      lastHandledRef.current = { raw, at: now };

      const url = new URL(raw);
      const pathname = normalizeSchemePath(url);

      switch (pathname) {
        case 'install-config':
        case 'subscribe-remote-profile': {
          const search = {
            subscribeUrl: url.searchParams.get('url') || undefined,
            subscribeName: decodeSearchParam(url.searchParams.get('name')),
            subscribeDesc: decodeSearchParam(url.searchParams.get('desc')),
          };

          if (windowLabel === 'main') {
            await navigate({
              to: '/main/profiles/$type',
              params: { type: 'profile' },
              search,
            } as never);
          } else {
            await navigate({
              to: '/profiles',
              search,
            } as never);
          }
          break;
        }
      }
    };

    const run = async () => {
      const unlisten = await listen<string>(
        'scheme-request-received',
        ({ payload }) => {
          void handleSchemeRequest(payload).catch((error) => {
            console.error(error);
          });
        },
      );

      if (disposed) {
        unlisten();
        return;
      }
      unlistenRef.current = unlisten;

      const pending = unwrapResult(await commands.getPendingDeepLink());
      if (pending) {
        await handleSchemeRequest(pending);
      }
    };

    run().catch((error) => {
      console.error(error);
    });

    return () => {
      disposed = true;
      unlistenRef.current?.();
      unlistenRef.current = null;
    };
  }, [navigate]);

  return null;
};

export default SchemeProvider;
