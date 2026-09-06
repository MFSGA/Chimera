import {
  commands,
  unwrapResult,
  useProfile,
  type PendingDeepLinkEntry,
} from '@chimera/interface';
import { useNavigate } from '@tanstack/react-router';
import { listen, type UnlistenFn } from '@tauri-apps/api/event';
import { getCurrentWebviewWindow } from '@tauri-apps/api/webviewWindow';
import { useEffect, useRef } from 'react';
import { Notice } from '@/components/base';
import * as m from '@/paraglide/messages';
import { formatError } from '@/utils';

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

export const SchemeProvider = () => {
  const navigate = useNavigate();
  const { create } = useProfile();
  const createRef = useRef(create);
  createRef.current = create;
  const unlistenRef = useRef<UnlistenFn | null>(null);

  useEffect(() => {
    const windowLabel = getCurrentWebviewWindow().label;
    if (!APP_WINDOW_LABELS.has(windowLabel)) {
      return;
    }

    let disposed = false;

    const handleSchemeRequest = async (raw: string) => {
      const url = new URL(raw);
      const pathname = normalizeSchemePath(url);

      switch (pathname) {
        case 'install-config': {
          const subscribeUrl = url.searchParams.get('url');
          if (!subscribeUrl) {
            Notice.error('Invalid install-config deep link', 3000);
            return;
          }

          try {
            const parsedSubscribeUrl = new URL(subscribeUrl);
            if (!['http:', 'https:'].includes(parsedSubscribeUrl.protocol)) {
              throw new Error('subscription URL must use http or https');
            }

            await createRef.current.mutateAsync({
              type: 'url',
              data: {
                url: subscribeUrl,
                name: decodeSearchParam(url.searchParams.get('name')) ?? null,
                option: null,
              },
            });
            Notice.success(m.profile_quick_import_success_message());
          } catch (error) {
            Notice.error(
              `Failed to import profile: ${formatError(error)}`,
              3000,
            );
          }
          break;
        }
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
      const unlisten = await listen<PendingDeepLinkEntry>(
        'scheme-request-received',
        ({ payload }) => {
          void (async () => {
            const claimed = unwrapResult(
              await commands.claimPendingDeepLink(payload.id),
            );
            if (claimed) {
              await handleSchemeRequest(payload.url);
            }
          })().catch((error) => {
            console.error(error);
          });
        },
      );

      if (disposed) {
        unlisten();
        return;
      }
      unlistenRef.current = unlisten;

      const pending = unwrapResult(await commands.getPendingDeepLinks());
      for (const entry of pending) {
        try {
          await handleSchemeRequest(entry.url);
        } catch (error) {
          console.error(error);
        }
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
