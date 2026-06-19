/**
 * Rules Provider 数据 hook
 *
 * 从 Clash HTTP API (GET /providers/rules) 获取 rule provider 列表，
 * 并为每个 provider 添加 mutate 方法以支持手动更新。
 *
 * 迁移自 ref: `ref/frontend/interface/src/ipc/use-clash-rules-provider.ts`
 * 差异：ref 使用 Tauri commands，Chimera 使用 Clash HTTP API（ofetch）
 */

import { useQuery, useQueryClient } from '@tanstack/react-query';
import { useClashAPI, type RuleProviderItem } from '../service/clash-api';
import { CLASH_RULES_PROVIDER_QUERY_KEY } from './consts';

export interface ClashRulesProviderQueryItem extends RuleProviderItem {
  mutate: () => Promise<void>;
}

export type ClashRulesProviderQuery = Record<
  string,
  ClashRulesProviderQueryItem
>;

export const useClashRulesProvider = () => {
  const { isReady, providersRules, providersRulesUpdate } = useClashAPI();
  const queryClient = useQueryClient();

  const query = useQuery({
    queryKey: [CLASH_RULES_PROVIDER_QUERY_KEY],
    queryFn: async () => {
      const result = await providersRules();

      if (!result?.providers) {
        return {} as ClashRulesProviderQuery;
      }

      const { providers } = result;

      return Object.fromEntries(
        Object.entries(providers).map(([key, value]) => [
          key,
          {
            ...value,
            mutate: async () => {
              await providersRulesUpdate(key);
              await queryClient.invalidateQueries({
                queryKey: [CLASH_RULES_PROVIDER_QUERY_KEY],
              });
            },
          },
        ]),
      ) as ClashRulesProviderQuery;
    },
    enabled: isReady,
  });

  return {
    ...query,
  };
};
