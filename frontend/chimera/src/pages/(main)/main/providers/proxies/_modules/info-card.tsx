import type { ClashProxiesProviderQueryItem } from '@chimera/interface';
import { Button } from '@chimera/ui/button';
import { Card, CardContent, CardFooter, CardHeader } from '@chimera/ui/card';
import MonitorHeartRounded from '~icons/material-symbols/monitor-heart-rounded';
import RefreshRounded from '~icons/material-symbols/refresh-rounded';
import { useLockFn } from '@/hooks/use-lock-fn';
import * as m from '@/paraglide/messages';
import { getLocale } from '@/paraglide/runtime';
import { formatRelativeTime } from '@/utils/date';
import { useProxiesProviderHealthcheck } from '../../_modules/use-proxies-provider-healthcheck';
import { useProxiesProviderUpdate } from '../../_modules/use-proxies-provider-update';

export const InfoCard = ({ data }: { data: ClashProxiesProviderQueryItem }) => {
  const blockTask = useProxiesProviderUpdate(data);
  const healthcheckTask = useProxiesProviderHealthcheck(data);

  const handleRefreshClick = useLockFn(async () => {
    try {
      await blockTask.execute();
    } catch {
      // The task already displayed its error dialog and retained error state.
    }
  });
  const handleHealthcheckClick = useLockFn(async () => {
    try {
      await healthcheckTask.execute();
    } catch {
      // A failed native health check must not be misreported as successful.
    }
  });

  return (
    <Card
      className="col-span-2 flex flex-col justify-between"
      data-slot="providers-proxy-info-card"
    >
      <CardHeader>{m.providers_info_title()}</CardHeader>

      <CardContent>
        <div className="flex items-center justify-between px-1">
          <div className="text-secondary text-sm">
            {m.providers_proxies_proxy_count_label({
              count: data.proxyCount,
            })}
          </div>

          <div className="text-sm text-zinc-500">
            {data.vehicleType}/{data.type}
          </div>
        </div>
      </CardContent>

      <CardFooter>
        <Button
          className="flex items-center gap-2"
          onClick={handleRefreshClick}
          loading={blockTask.isPending}
          disabled={healthcheckTask.isPending}
          data-slot="providers-detail-update"
          data-error={String(blockTask.isError)}
        >
          <RefreshRounded />
          <span>{m.providers_update_provider()}</span>
        </Button>
        <Button
          className="flex items-center gap-2"
          onClick={handleHealthcheckClick}
          loading={healthcheckTask.isPending}
          disabled={blockTask.isPending}
          data-slot="providers-detail-healthcheck"
          data-error={String(healthcheckTask.isError)}
        >
          <MonitorHeartRounded />
          <span>{m.providers_healthcheck_provider()}</span>
        </Button>

        <div className="flex-1" />

        <div
          className="hover:bg-surface-variant text-secondary rounded-full px-3 py-2 text-xs font-semibold"
          data-slot="providers-detail-updated-at"
        >
          {m.profile_subscription_updated_at({
            updated: formatRelativeTime(
              data.updatedAt,
              Date.now(),
              getLocale(),
            ),
          })}
        </div>
      </CardFooter>
    </Card>
  );
};
