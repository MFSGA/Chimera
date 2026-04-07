import { QueryClient, QueryClientProvider } from '@tanstack/react-query';
import type { PropsWithChildren } from 'react';
import { ClashWSProvider, useClashWSContext } from './clash-ws-provider';
import { MutationProvider } from './mutation-provider';

const queryClient = new QueryClient();

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
