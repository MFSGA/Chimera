import { QueryClient, QueryClientProvider } from '@tanstack/react-query';
import type { PropsWithChildren } from 'react';
import { ClashWSProvider } from './clash-ws-provider';

const queryClient = new QueryClient();

// todo: add more providers here
export const RootProvider: any = ({ children }: PropsWithChildren) => {
  return (
    <QueryClientProvider client={queryClient}>
      <ClashWSProvider>{children}</ClashWSProvider>
    </QueryClientProvider>
  );
};
