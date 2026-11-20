import { useMutation, useQuery } from '@tanstack/react-query';
import { unwrapResult } from '../utils';
import { commands } from './bindings';

/** Ref-aligned static provider list; no outbound request until the user opts in. */
export const useIpLookupProviders = () =>
  useQuery({
    queryKey: ['getIpLookupProviders'],
    queryFn: () => commands.getIpLookupProviders(),
  });

/** An explicit STUN-server test checks UDP/TCP × IPv4/IPv6. */
export const useStunServerTest = () =>
  useMutation({
    mutationKey: ['testStunServer'],
    mutationFn: async ({ host, port }: { host: string; port: number }) =>
      unwrapResult(await commands.testStunServer(host, port)),
    retry: false,
  });
