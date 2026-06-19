/**
 * Proxies Provider Subscription 卡片
 *
 * 迁移自 ref: `ref/frontend/nyanpasu/src/pages/(main)/main/providers/proxies/_modules/subscription-card.tsx`
 *
 * 显示 proxy provider 的订阅使用量（进度条 + 已用/总量）
 * 如果没有 subscriptionInfo 则不显示
 */

import type { ClashProxiesProviderQueryItem } from '@chimera/interface';
import { filesize } from 'filesize';
import { Card, CardContent, CardHeader } from '@/components/ui/card';
import { LinearProgress } from '@/components/ui/linear-progress';
import * as m from '@/paraglide/messages';
import { useProxiesSubscription } from '../../_modules/use-proxies-subscription';

export const SubscriptionCard = ({
  data,
}: {
  data: ClashProxiesProviderQueryItem;
}) => {
  const { progress, total, used, hasSubscriptionInfo } =
    useProxiesSubscription(data);

  if (!hasSubscriptionInfo) {
    return null;
  }

  return (
    <Card className="col-span-2 flex flex-col justify-between">
      <CardHeader>{m.providers_subscription_title()}</CardHeader>

      <CardContent>
        <LinearProgress value={progress} />

        <div className="flex items-center justify-between pb-2">
          <div className="text-sm font-bold">{progress.toFixed(2)}%</div>

          <div className="text-sm font-bold">
            {filesize(used)} / {filesize(total)}
          </div>
        </div>
      </CardContent>
    </Card>
  );
};
