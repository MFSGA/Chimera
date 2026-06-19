/**
 * Providers 主页面
 *
 * 迁移自 ref: `ref/frontend/nyanpasu/src/pages/(main)/main/providers/index.tsx`
 *
 * 显示 Proxy Providers 和 Rules Providers 分组列表
 * 每个 provider 显示名称、类型、更新时间、资源数量
 * 支持单独更新或全部更新
 */

import {
  useClashProxiesProvider,
  useClashRulesProvider,
  type ClashProxiesProviderQueryItem,
  type ClashRulesProviderQueryItem,
} from '@chimera/interface';
import { cn } from '@chimera/ui';
import { createFileRoute, Link } from '@tanstack/react-router';
import dayjs from 'dayjs';
import { filesize } from 'filesize';
import type { ComponentProps, PropsWithChildren } from 'react';
import { useBlockTask } from '@/components/providers/block-task-provider';
import { Button } from '@/components/ui/button';
import { Card, CardContent } from '@/components/ui/card';
import { LinearProgress } from '@/components/ui/linear-progress';
import TextMarquee from '@/components/ui/text-marquee';
import * as m from '@/paraglide/messages';
import { formatError } from '@/utils';
import { message } from '@/utils/notification';
import { useProxiesProviderUpdate } from './_modules/use-proxies-provider-update';
import { useProxiesSubscription } from './_modules/use-proxies-subscription';
import { useRulesProviderUpdate } from './_modules/use-rules-provider-update';

export const Route = createFileRoute('/(main)/main/providers/')({
  component: RouteComponent,
});

/** 全部更新图标 SVG */
function RefreshAllIcon() {
  return (
    <svg
      className="size-5"
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

/** 单个刷新图标 SVG */
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

/** 空列表图标 SVG */
function AllInboxIcon() {
  return (
    <svg
      className="size-10"
      viewBox="0 0 24 24"
      fill="none"
      stroke="currentColor"
      strokeWidth="1.5"
    >
      <path d="M21 9v10a2 2 0 0 1-2 2H5a2 2 0 0 1-2-2V9M3 9V5a2 2 0 0 1 2-2h14a2 2 0 0 1 2 2v4" />
      <path d="M3 9h18" />
      <path d="M15 13H9" />
    </svg>
  );
}

const NavigateButton = ({
  className,
  ...props
}: ComponentProps<typeof Button>) => {
  return (
    <Button
      variant="fab"
      className={cn(
        'flex h-auto w-full flex-col justify-center gap-1 p-3 text-left',
        'bg-on-background/3!',
        'dark:bg-surface!',
        'shadow-none',
        'hover:shadow-none',
        'hover:bg-surface-variant/30',
        className,
      )}
      asChild
      {...props}
    />
  );
};

const Group = ({ children }: PropsWithChildren) => {
  return (
    <div className="flex flex-col gap-1" data-slot="providers-group">
      {children}
    </div>
  );
};

const GroupTitle = ({ children }: PropsWithChildren) => {
  return (
    <div
      className={cn(
        'sticky top-0 z-10 pl-1 text-lg font-semibold',
        'text-secondary bg-mixed-background flex h-16 items-center justify-between',
      )}
      data-slot="providers-group-title"
    >
      {children}
    </div>
  );
};

const GroupContent = ({ children }: PropsWithChildren) => {
  return (
    <div
      className="grid grid-cols-2 gap-2 sm:grid-cols-3 md:grid-cols-4"
      data-slot="providers-group-content"
    >
      {children}
    </div>
  );
};

const Empty = ({ children }: PropsWithChildren) => {
  return (
    <Card variant="outline">
      <CardContent className="min-h-40 items-center justify-center text-sm">
        <AllInboxIcon />

        {children}
      </CardContent>
    </Card>
  );
};

const Proxies = ({ data }: { data: ClashProxiesProviderQueryItem }) => {
  const { progress, total, used, hasSubscriptionInfo } =
    useProxiesSubscription(data);

  const blockTask = useProxiesProviderUpdate(data);

  const handleClick = blockTask.execute;

  return (
    <NavigateButton className="flex flex-col gap-2">
      <Link
        to="/main/providers/proxies/$key"
        params={{
          key: data.name,
        }}
      >
        <div className="flex items-center justify-between gap-2">
          <TextMarquee className="text-sm font-medium">{data.name}</TextMarquee>

          <div className="text-xs text-nowrap text-zinc-700 dark:text-zinc-300">
            {data.updatedAt ? dayjs(data.updatedAt).fromNow() : '-'}
          </div>
        </div>

        <div className="text-xs text-zinc-500">
          {data.vehicleType}/{data.type}
        </div>

        <div className="flex flex-1 flex-col gap-2 text-xs text-zinc-500">
          {hasSubscriptionInfo && (
            <>
              <LinearProgress value={progress} />

              <TextMarquee>
                <div className="flex items-center justify-between gap-2 text-xs font-bold">
                  <div>{progress.toFixed(2)}%</div>

                  <div>
                    {filesize(used)} / {filesize(total)}
                  </div>
                </div>
              </TextMarquee>
            </>
          )}
        </div>

        <div className="flex items-center justify-between">
          <div className="bg-surface-variant text-secondary rounded-full px-2 py-1 text-[10px]">
            {m.providers_proxies_proxy_count_label({
              count: data.proxies.length,
            })}
          </div>

          <Button
            className="size-6"
            icon
            loading={blockTask.isPending}
            onClick={(e) => {
              e.preventDefault();
              e.stopPropagation();
              handleClick();
            }}
          >
            <RefreshIcon />
          </Button>
        </div>
      </Link>
    </NavigateButton>
  );
};

const Rules = ({ data }: { data: ClashRulesProviderQueryItem }) => {
  const blockTask = useRulesProviderUpdate(data);

  const handleClick = blockTask.execute;

  return (
    <NavigateButton className="flex flex-col gap-2">
      <Link
        to="/main/providers/rules/$key"
        params={{
          key: data.name,
        }}
      >
        <div className="flex items-center justify-between gap-2">
          <TextMarquee className="text-sm font-medium">{data.name}</TextMarquee>

          <div className="text-xs text-nowrap text-zinc-700 dark:text-zinc-300">
            {data.updatedAt ? dayjs(data.updatedAt).fromNow() : '-'}
          </div>
        </div>

        <div className="text-xs text-zinc-500">
          {data.vehicleType}/{data.type}
        </div>

        <div className="flex items-center justify-between">
          <div className="bg-surface-variant text-secondary rounded-full px-2 py-1 text-[10px]">
            {m.providers_rules_rule_count_label({
              count: data.ruleCount ?? 0,
            })}
          </div>

          <Button
            className="size-6"
            icon
            loading={blockTask.isPending}
            onClick={(e) => {
              e.preventDefault();
              e.stopPropagation();
              handleClick();
            }}
          >
            <RefreshIcon />
          </Button>
        </div>
      </Link>
    </NavigateButton>
  );
};

function RouteComponent() {
  const proxiesProvider = useClashProxiesProvider();
  const rulesProvider = useClashRulesProvider();

  const proxiesLoading = proxiesProvider.isLoading;
  const proxies = proxiesProvider.data
    ? Object.entries(proxiesProvider.data)
    : null;

  const rulesLoading = rulesProvider.isLoading;
  const rules = rulesProvider.data ? Object.entries(rulesProvider.data) : null;

  const proxiesBlockTask = useBlockTask('update-proxies-provider', async () => {
    if (!proxies) {
      return;
    }

    try {
      await Promise.all(proxies.map(([_, data]) => data.mutate()));
    } catch (error) {
      console.error('Failed to update proxies provider', error);
      await message(`Update provider failed: \n ${formatError(error)}`, {
        title: 'Error',
        kind: 'error',
      });
    }
  });

  const handleUpdateProxies = proxiesBlockTask.execute;

  const rulesBlockTask = useBlockTask('update-rules-provider', async () => {
    if (!rules) {
      return;
    }

    try {
      await Promise.all(rules.map(([_, data]) => data.mutate()));
    } catch (error) {
      console.error('Failed to update rules provider', error);
      await message(`Update provider failed: \n ${formatError(error)}`, {
        title: 'Error',
        kind: 'error',
      });
    }
  });

  const handleUpdateRules = rulesBlockTask.execute;

  return (
    <div className="flex flex-col gap-4 p-4 pt-0">
      <Group>
        <GroupTitle>
          <span>{m.providers_proxies_title()}</span>

          <Button
            icon
            onClick={handleUpdateProxies}
            loading={proxiesBlockTask.isPending}
          >
            <RefreshAllIcon />
          </Button>
        </GroupTitle>

        {proxiesLoading ? (
          <GroupContent>
            {Array.from({ length: 6 }).map((_, i) => (
              <Card key={i} variant="outline">
                <CardContent className="bg-on-background/5 h-32 animate-pulse" />
              </Card>
            ))}
          </GroupContent>
        ) : proxies && proxies.length ? (
          <GroupContent>
            {proxies.map(([key, data]) => (
              <Proxies key={key} data={data} />
            ))}
          </GroupContent>
        ) : (
          <Empty>
            <p>{m.providers_no_proxies_message()}</p>
          </Empty>
        )}
      </Group>

      <Group>
        <GroupTitle>
          <span>{m.providers_rules_title()}</span>

          <Button
            icon
            onClick={handleUpdateRules}
            loading={rulesBlockTask.isPending}
          >
            <RefreshAllIcon />
          </Button>
        </GroupTitle>

        {rulesLoading ? (
          <GroupContent>
            {Array.from({ length: 6 }).map((_, i) => (
              <Card key={i} variant="outline">
                <CardContent className="bg-on-background/5 h-32 animate-pulse" />
              </Card>
            ))}
          </GroupContent>
        ) : rules && rules.length ? (
          <GroupContent>
            {rules.map(([key, data]) => (
              <Rules key={key} data={data} />
            ))}
          </GroupContent>
        ) : (
          <Empty>
            <p>{m.providers_no_rules_message()}</p>
          </Empty>
        )}
      </Group>
    </div>
  );
}
