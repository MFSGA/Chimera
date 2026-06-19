/**
 * Proxy Provider 数据 hook
 *
 * 从 Clash HTTP API (GET /providers/proxies) 获取 proxy provider 列表，
 * 并为每个 provider 添加 mutate 方法以支持手动更新。
 *
 * 迁移自 ref: `ref/frontend/interface/src/ipc/use-clash-proxies-provider.ts`
 * 差异：ref 使用 Tauri commands，Chimera 使用 Clash HTTP API（ofetch）
 */

import { useQuery, useQueryClient } from '@tanstack/react-query';
import { useClashAPI, type ProxyProviderItem } from '../service/clash-api';
import { CLASH_PROXIES_PROVIDER_QUERY_KEY } from './consts';

export interface ClashProxiesProviderQueryItem extends ProxyProviderItem {
  mutate: () => Promise<void>;
}

export type ClashProxiesProviderQuery = Record<
  string,
  ClashProxiesProviderQueryItem
>;

export const useClashProxiesProvider = () => {
  const { isReady, providersProxies, providersProxiesUpdate } = useClashAPI();
  const queryClient = useQueryClient();

  const query = useQuery({
    queryKey: [CLASH_PROXIES_PROVIDER_QUERY_KEY],
    queryFn: async () => {
      const result = await providersProxies();

      if (!result?.providers) {
        return {} as ClashProxiesProviderQuery;
      }

      const { providers } = result;

      return Object.fromEntries(
        Object.entries(providers)
          .filter(([, value]) =>
            ['http', 'file'].includes(value.vehicleType.toLowerCase()),
          )
          .map(([key, value]) => [
            key,
            {
              ...value,
              mutate: async () => {
                await providersProxiesUpdate(key);
                await queryClient.invalidateQueries({
                  queryKey: [CLASH_PROXIES_PROVIDER_QUERY_KEY],
                });
              },
            },
          ]),
      ) as ClashProxiesProviderQuery;
    },
    enabled: isReady,
  });

  return {
    ...query,
  };
};
