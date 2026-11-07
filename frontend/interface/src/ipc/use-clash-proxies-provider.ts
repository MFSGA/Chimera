import { useMutation, useQuery, useQueryClient } from '@tanstack/react-query';
import { unwrapResult } from '../utils';
import type { ProxyProviderSummary_Serialize } from './bindings';
import { mutations, queries } from './query-bindings.js';
import { invokeMutation, invokeQuery } from './query-options.js';

export type ProxyProviderItem = ProxyProviderSummary_Serialize;
export type ClashProxiesProviderQueryItem = ProxyProviderSummary_Serialize;
export type ClashProxiesProviderQuery = Record<
  string,
  ClashProxiesProviderQueryItem
>;

export const useClashProxiesProvider = (
  options: { refetchInterval?: number | false; enabled?: boolean } = {},
) => {
  const providerQuery = queries.clashApiGetProvidersProxies();
  return useQuery({
    ...options,
    queryKey: providerQuery.queryKey,
    queryFn: async (): Promise<ClashProxiesProviderQuery> => {
      const providers = unwrapResult(await invokeQuery(providerQuery));
      return Object.fromEntries(
        Object.entries(providers).filter(([, value]) =>
          ['http', 'file'].includes(value.vehicleType.toLowerCase()),
        ),
      );
    },
  });
};

export const useUpdateClashProxiesProvider = () => {
  const queryClient = useQueryClient();
  const update = mutations.updateProxyProvider;
  return useMutation({
    mutationKey: update.mutationKey,
    mutationFn: async (name: string) =>
      unwrapResult(await invokeMutation(update, [name])),
    // The provider may add/remove proxies. Refresh both graph schemas too,
    // since provider updates are not guaranteed to emit a proxy-state event.
    onSuccess: () =>
      Promise.all([
        queryClient.invalidateQueries({
          queryKey: queries.clashApiGetProvidersProxies().queryKey,
        }),
        queryClient.invalidateQueries({
          queryKey: queries.getProxies().queryKey,
        }),
        queryClient.invalidateQueries({
          queryKey: queries.getNativeProxies().queryKey,
        }),
      ]),
  });
};
