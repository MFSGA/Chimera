import {
  commands,
  unwrapResult,
  useProfile,
  type IVerge_Deserialize,
  type PendingDeepLinkEntry,
} from '@chimera/interface';
import { useNavigate } from '@tanstack/react-router';
import { listen, type UnlistenFn } from '@tauri-apps/api/event';
import { getCurrentWebviewWindow } from '@tauri-apps/api/webviewWindow';
import { useEffect, useRef } from 'react';
import { Notice } from '@/components/base';
import { parseDeepLink } from '@/features/deep-link/parser';
import {
  applySettingsDeepLink,
  type DeepLinkSettingsGateway,
} from '@/features/deep-link/settings';
import * as m from '@/paraglide/messages';
import { setLocale } from '@/paraglide/runtime';
import { formatError } from '@/utils';

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

    const settingsGateway: DeepLinkSettingsGateway = {
      getSystemProxyEnabled: async () => {
        const config = unwrapResult(await commands.getVergeConfig());
        return config.enable_system_proxy ?? false;
      },
      setSystemProxyEnabled: async (enabled) => {
        unwrapResult(
          await commands.patchVergeConfig({
            enable_system_proxy: enabled,
          } as unknown as IVerge_Deserialize),
        );
      },
      setLanguage: async (locale) => {
        unwrapResult(
          await commands.patchVergeConfig({
            language: locale,
          } as unknown as IVerge_Deserialize),
        );
        setLocale(locale);
      },
    };

    const handleSchemeRequest = async (raw: string) => {
      const command = parseDeepLink(raw);

      switch (command.type) {
        case 'system-proxy':
        case 'language':
          await applySettingsDeepLink(command, settingsGateway);
          return;
        case 'install-config':
          await createRef.current.mutateAsync({
            type: 'url',
            data: {
              url: command.url,
              name: command.name ?? null,
              option: null,
            },
          });
          Notice.success(m.profile_quick_import_success_message());
          return;
        case 'subscribe-remote-profile': {
          const search = {
            subscribeUrl: command.url,
            subscribeName: command.name,
            subscribeDesc: command.description,
          };
          await navigate(
            windowLabel === 'main'
              ? ({
                  to: '/main/profiles/$type',
                  params: { type: 'profile' },
                  search,
                } as never)
              : ({ to: '/profiles', search } as never),
          );
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
            console.error('[deep-link] failed to handle request', error);
            Notice.error(
              `Failed to handle deep link: ${formatError(error)}`,
              3000,
            );
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
          const claimed = unwrapResult(
            await commands.claimPendingDeepLink(entry.id),
          );
          if (claimed) {
            await handleSchemeRequest(entry.url);
          }
        } catch (error) {
          console.error('[deep-link] failed to handle pending request', error);
          Notice.error(
            `Failed to handle deep link: ${formatError(error)}`,
            3000,
          );
        }
      }
    };

    run().catch((error) => {
      console.error('[deep-link] initialization failed', error);
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
