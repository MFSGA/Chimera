/**
 * Proxies Provider 详情页面
 *
 * 迁移自 ref: `ref/frontend/nyanpasu/src/pages/(main)/main/providers/proxies/$key.tsx`
 *
 * 显示单个 proxy provider 的详细信息，
 * 包含订阅使用量卡片和信息卡片
 */

import { useClashProxiesProvider } from '@chimera/interface';
import { createFileRoute } from '@tanstack/react-router';
import { ProvidersTitle } from '../_modules/providers-title';
import { InfoCard } from './_modules/info-card';
import { SubscriptionCard } from './_modules/subscription-card';

export const Route = createFileRoute('/(main)/main/providers/proxies/$key')({
  component: RouteComponent,
});

function RouteComponent() {
  const { key } = Route.useParams();

  const proxiesProvider = useClashProxiesProvider();

  const currentProxy = proxiesProvider.data?.[key];

  if (!currentProxy) {
    return null;
  }

  return (
    <>
      <ProvidersTitle>{key}</ProvidersTitle>

      <div className="grid grid-cols-2 gap-4 p-4 md:grid-cols-4">
        <SubscriptionCard data={currentProxy} />

        <InfoCard data={currentProxy} />
      </div>
    </>
  );
}
