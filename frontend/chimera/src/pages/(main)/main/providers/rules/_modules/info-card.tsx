/**
 * Rules Provider Info 卡片
 *
 * 迁移自 ref: `ref/frontend/nyanpasu/src/pages/(main)/main/providers/rules/_modules/info-card.tsx`
 *
 * 显示 rule provider 的规则数量、类型、更新时间，并提供更新按钮
 */

import type { ClashRulesProviderQueryItem } from '@chimera/interface';
import dayjs from 'dayjs';
import { Button } from '@/components/ui/button';
import {
  Card,
  CardContent,
  CardFooter,
  CardHeader,
} from '@/components/ui/card';
import * as m from '@/paraglide/messages';
import { useRulesProviderUpdate } from '../../_modules/use-rules-provider-update';

/** 刷新图标 SVG */
function RefreshIcon() {
  return (
    <svg
      className="size-4"
      viewBox="0 0 24 24"
      fill="none"
      stroke="currentColor"
      strokeWidth="2"
    >
      <path d="M21 3v6h-6M3 21v-6h6" />
      <path d="M3.05 11A9 9 0 0 1 20.3 6.3L21 6M20.95 13A9 9 0 0 1 3.7 17.7L3 18" />
    </svg>
  );
}

export const InfoCard = ({ data }: { data: ClashRulesProviderQueryItem }) => {
  const blockTask = useRulesProviderUpdate(data);

  const handleRefreshClick = blockTask.execute;

  return (
    <Card className="col-span-2 flex flex-col justify-between">
      <CardHeader>{m.providers_info_title()}</CardHeader>

      <CardContent>
        <div className="flex items-center justify-between px-1">
          <div className="text-secondary text-sm">
            {m.providers_rules_rule_count_label({
              count: data.ruleCount ?? 0,
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
        >
          <RefreshIcon />
          <span>{m.providers_update_provider()}</span>
        </Button>

        <div className="flex-1" />

        <div className="hover:bg-surface-variant text-secondary rounded-full px-3 py-2 text-xs font-semibold">
          {m.profile_subscription_updated_at({
            updated: data.updatedAt ? dayjs(data.updatedAt).fromNow() : '-',
          })}
        </div>
      </CardFooter>
    </Card>
  );
};
