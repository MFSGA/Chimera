import { useQueryClient } from '@tanstack/react-query';
import { listen, type UnlistenFn } from '@tauri-apps/api/event';
import { useEffect, useRef, type PropsWithChildren } from 'react';
import {
  CHIMERA_BACKEND_EVENT_NAME,
  CHIMERA_SETTING_QUERY_KEY,
  CHIMERA_SYSTEM_PROXY_QUERY_KEY,
  CLASH_CONFIG_QUERY_KEY,
  CLASH_INFO_QUERY_KEY,
  CLASH_PROXIES_QUERY_KEY,
  CLASH_VERSION_QUERY_KEY,
  RROFILES_QUERY_KEY,
} from '../ipc/consts';

type EventPayload = 'nyanpasu_config' | 'clash_config' | 'profiles' | 'proxies';

const CHIMERA_CONFIG_MUTATION_KEYS = [
  CHIMERA_SETTING_QUERY_KEY,
  CHIMERA_SYSTEM_PROXY_QUERY_KEY,
] as const;

const CLASH_CONFIG_MUTATION_KEYS = [
  CLASH_VERSION_QUERY_KEY,
  CLASH_INFO_QUERY_KEY,
  CLASH_CONFIG_QUERY_KEY,
  RROFILES_QUERY_KEY,
] as const;

const PROFILES_MUTATION_KEYS = [RROFILES_QUERY_KEY] as const;

const PROXIES_MUTATION_KEYS = [CLASH_PROXIES_QUERY_KEY] as const;

export const MutationProvider = ({ children }: PropsWithChildren) => {
  const unlistenRef = useRef<UnlistenFn | null>(null);
  const queryClient = useQueryClient();

  useEffect(() => {
    const invalidateQueries = (keys: readonly string[]) => {
      Promise.all(
        keys.map((key) =>
          queryClient.invalidateQueries({
            queryKey: [key],
          }),
        ),
      ).catch((error) => {
        console.error(error);
      });
    };

    listen<EventPayload>(CHIMERA_BACKEND_EVENT_NAME, ({ payload }) => {
      switch (payload) {
        case 'nyanpasu_config':
          invalidateQueries(CHIMERA_CONFIG_MUTATION_KEYS);
          break;
        case 'clash_config':
          invalidateQueries(CLASH_CONFIG_MUTATION_KEYS);
          break;
        case 'profiles':
          invalidateQueries(PROFILES_MUTATION_KEYS);
          break;
        case 'proxies':
          invalidateQueries(PROXIES_MUTATION_KEYS);
          break;
      }
    })
      .then((unlisten) => {
        unlistenRef.current = unlisten;
      })
      .catch((error) => {
        console.error(error);
      });

    return () => {
      unlistenRef.current?.();
    };
  }, [queryClient]);

  return children;
};
