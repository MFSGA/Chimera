import { useMutation, useQuery, useQueryClient } from '@tanstack/react-query';
import { unwrapResult } from '../utils';
import type { RuleProviderItem } from './bindings';
import { mutations, queries } from './query-bindings.js';
import { invokeMutation, invokeQuery } from './query-options.js';

export type ClashRulesProviderQueryItem = RuleProviderItem;
export type ClashRulesProviderQuery = Record<
  string,
  ClashRulesProviderQueryItem
>;

export const useClashRulesProvider = (
  options: { refetchInterval?: number | false; enabled?: boolean } = {},
) => {
  const providerQuery = queries.clashApiGetProvidersRules();
  return useQuery({
    ...options,
    queryKey: providerQuery.queryKey,
    queryFn: async (): Promise<ClashRulesProviderQuery> =>
      unwrapResult(await invokeQuery(providerQuery)).providers,
  });
};

export const useUpdateClashRulesProvider = () => {
  const queryClient = useQueryClient();
  const update = mutations.clashApiUpdateProvidersRules;
  return useMutation({
    mutationKey: update.mutationKey,
    mutationFn: async (name: string) =>
      unwrapResult(await invokeMutation(update, [name])),
    onSuccess: () =>
      queryClient.invalidateQueries({
        queryKey: queries.clashApiGetProvidersRules().queryKey,
      }),
  });
};
