import {
  MutationCache,
  QueryClient,
  QueryClientProvider,
} from '@tanstack/react-query';
import type { PropsWithChildren } from 'react';
import type { Degradation } from '../ipc/bindings';
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

export const RootProvider: any = ({ children }: PropsWithChildren) => {
  return (
    <QueryClientProvider client={queryClient}>
      <MutationProvider>
        <ClashWSProvider>{children}</ClashWSProvider>
      </MutationProvider>
    </QueryClientProvider>
  );
};

export { useClashWSContext };
