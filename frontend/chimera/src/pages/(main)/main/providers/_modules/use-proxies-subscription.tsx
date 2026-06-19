/**
 * Proxy Provider 订阅信息计算 hook
 *
 * 迁移自 ref: `ref/frontend/nyanpasu/src/pages/(main)/main/providers/_modules/use-proxies-subscription.tsx`
 *
 * 从 proxy provider 数据中提取 subscriptionInfo，
 * 计算使用量进度（百分比），
 * 返回 progress / total / used / hasSubscriptionInfo
 */

import type { ProxyProviderItem } from '@chimera/interface';
import { useMemo } from 'react';

const clampPercentage = (value: number) => Math.min(100, Math.max(0, value));

export const useProxiesSubscription = (data: ProxyProviderItem) => {
  return useMemo(() => {
    let progress = 0;
    let total = 0;
    let used = 0;

    const subscriptionInfo =
      'subscriptionInfo' in data ? data.subscriptionInfo : undefined;
    const hasSubscriptionInfo =
      subscriptionInfo !== null && subscriptionInfo !== undefined;

    if (hasSubscriptionInfo && subscriptionInfo) {
      // Some Clash cores (Mihomo) return PascalCase keys; be defensive
      const si = subscriptionInfo as Record<string, number | undefined>;
      const download = si.download ?? si.Download ?? 0;
      const upload = si.upload ?? si.Upload ?? 0;
      total = si.total ?? si.Total ?? 0;
      used = download + upload;

      if (total > 0) {
        progress = clampPercentage((used / total) * 100);
      }
    }

    return {
      progress,
      total,
      used,
      hasSubscriptionInfo,
    };
  }, [data]);
};
