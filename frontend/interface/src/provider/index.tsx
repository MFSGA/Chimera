import {
  MutationCache,
  QueryClient,
  QueryClientProvider,
} from '@tanstack/react-query';
import { listen, type UnlistenFn } from '@tauri-apps/api/event';
import { useEffect, type PropsWithChildren } from 'react';
import type { Degradation } from '../ipc/bindings';
import {
  CHIMERA_SETTING_QUERY_KEY,
  CHIMERA_SETTING_UPDATED_EVENT,
} from '../ipc/consts';
import { ClashWSProvider, useClashWSContext } from './clash-ws-provider';
import { MutationProvider } from './mutation-provider';

let mutationDegradationHandler: ((degradations: Degradation[]) => void) | null =
  null;

export const setMutationDegradationHandler = (
  handler: (degradations: Degradation[]) => void,
) => {
  mutationDegradationHandler = handler;
  return () => {
    if (mutationDegradationHandler === handler) {
      mutationDegradationHandler = null;
    }
  };
};

const queryClient = new QueryClient({
  mutationCache: new MutationCache({
    onSuccess: (data) => {
      if (
        !data ||
        typeof data !== 'object' ||
        !('status' in data) ||
        (data as { status?: unknown }).status !== 'committed_degraded'
      ) {
        return;
      }

      const degradations = (data as { degradations?: unknown }).degradations;
      if (Array.isArray(degradations)) {
        mutationDegradationHandler?.(degradations as Degradation[]);
      }
    },
  }),
});

const SettingSyncProvider = ({ children }: PropsWithChildren) => {
  useEffect(() => {
    let disposed = false;
    let unlisten: UnlistenFn | undefined;

    listen(CHIMERA_SETTING_UPDATED_EVENT, () => {
      void queryClient.invalidateQueries({
        queryKey: [CHIMERA_SETTING_QUERY_KEY],
      });
    })
      .then((stop) => {
        if (disposed) stop();
        else unlisten = stop;
      })
      .catch((error) => {
        console.error('[settings] failed to listen for config updates', error);
      });

    return () => {
      disposed = true;
      unlisten?.();
    };
  }, []);

  return children;
};

export const RootProvider: any = ({ children }: PropsWithChildren) => {
  return (
    <QueryClientProvider client={queryClient}>
      <SettingSyncProvider>
        <MutationProvider>
          <ClashWSProvider>{children}</ClashWSProvider>
        </MutationProvider>
      </SettingSyncProvider>
    </QueryClientProvider>
  );
};

export { useClashWSContext };
