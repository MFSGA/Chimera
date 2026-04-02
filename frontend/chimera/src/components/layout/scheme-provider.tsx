import { useNavigate } from '@tanstack/react-router';
import { listen, type UnlistenFn } from '@tauri-apps/api/event';
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

export const SchemeProvider = () => {
  const navigate = useNavigate();
  const unlistenRef = useRef<UnlistenFn | null>(null);

  useEffect(() => {
    const run = async () => {
      unlistenRef.current = await listen<string>(
        'scheme-request-received',
        ({ payload }) => {
          const url = new URL(payload);
          const pathname = normalizeSchemePath(url);

          switch (pathname) {
            case 'install-config':
            case 'subscribe-remote-profile':
              navigate({
                to: '/profiles',
                search: {
                  subscribeUrl: url.searchParams.get('url') || undefined,
                  subscribeName: decodeSearchParam(
                    url.searchParams.get('name'),
                  ),
                  subscribeDesc: decodeSearchParam(
                    url.searchParams.get('desc'),
                  ),
                } as never,
              });
              break;
          }
        },
      );
    };

    run().catch((error) => {
      console.error(error);
    });

    return () => {
      unlistenRef.current?.();
    };
  }, [navigate]);

  return null;
};

export default SchemeProvider;
