/**
 * 代理节点按钮组件
 *
 * 迁移自 ref: `src/pages/(main)/main/proxies/group/_modules/proxy-node-button.tsx`
 *
 * 职责：
 * - 显示单个代理节点的名称和延迟测试结果
 * - 点击节点可切换选择（mutateSelect）
 * - 右侧嵌入的延迟按钮可单独测试该节点延迟（mutateDelay）
 * - 延迟结果使用 DelayChip 显示（绿色/黄色/红色）
 *
 * 当前阶段（Proxies Step 3 — 组详情迁移）：
 * - 匹配 ref 的 ProxyNodeButton 设计（含 per-node 延迟测试）
 * - 使用 useBlockTask 管理单个节点的延迟测试状态
 * - 使用 DelayChip 组件显示延迟值（复用了 Chimera 现有组件但 API 不同，
 *   此处直接使用 Button + 自定义延迟显示以匹配 ref 设计）
 * - 当前节点高亮使用 data-active 属性（匹配 ref 的 group-data-[active] 模式）
 *
 * 后续计划：
 * - 考虑复用 ref 的 DelayChip 设计，或统一 Chimera 的 DelayChip 组件
 */

import { type ClashProxiesQueryProxyItem } from '@chimera/interface';
import { cn } from '@chimera/ui';
import FlashOnRounded from '~icons/material-symbols/flash-on-rounded';
import { useMemo, type ComponentProps, type MouseEvent } from 'react';
import { useBlockTask } from '@/components/providers/block-task-provider';
import { Button } from '@/components/ui/button';
import { useLockFn } from '@/hooks/use-lock-fn';

/**
 * 延迟测试结果的背景色映射
 *
 * 匹配 ref 的延迟阈值设计：
 * - <= 200ms：绿色（优秀）
 * - <= 500ms：黄色（良好）
 * - > 500ms：红色（较差）
 * - -1 或未测试：默认
 */
function delayColorClass(delay: number): string {
  if (delay <= 0) return ''; // 未测试
  if (delay <= 200) return 'bg-green-500/20 dark:bg-green-700/30';
  if (delay <= 500) return 'bg-yellow-500/20 dark:bg-yellow-700/30';
  return 'bg-red-500/20 dark:bg-red-700/30';
}

/**
 * 代理节点按钮
 *
 * 点击节点主体 → 切换选择该节点（mutateSelect）
 * 点击延迟按钮 → 测试该节点延迟（mutateDelay）
 *
 * @param props.proxy - 代理节点数据
 */
export default function ProxyNodeButton({
  proxy,
  ...props
}: Omit<ComponentProps<typeof Button>, 'onClick' | 'children'> & {
  proxy: ClashProxiesQueryProxyItem;
}) {
  // 切换选择当前节点
  const handleSelectProxy = useLockFn(async () => {
    await proxy.mutateSelect();
  });

  // 单节点延迟测试
  const delayTask = useBlockTask(
    `proxy-delay-check-${proxy.name.toLowerCase()}`,
    async () => {
      await proxy.mutateDelay();
    },
  );

  // 延迟按钮点击：阻止冒泡（不触发节点选择），执行延迟测试
  const handleDelayClick = useLockFn(
    async (e: MouseEvent<HTMLButtonElement>) => {
      e.preventDefault();
      e.stopPropagation();

      await delayTask.execute();
    },
  );

  // 获取最新延迟历史
  const currentDelay = useMemo(() => {
    if (!proxy.history || proxy.history.length === 0) {
      return -1;
    }
    return proxy.history[proxy.history.length - 1].delay;
  }, [proxy.history]);

  return (
    <Button
      variant="fab"
      className={cn(
        'flex w-full flex-col justify-center gap-1 px-2 text-left',
        'group-data-[active=true]:bg-primary-container/75',
        'dark:group-data-[active=true]:bg-surface-variant/50',
        'group-data-[active=false]:bg-on-background/3',
        'dark:group-data-[active=false]:bg-surface/30',
        'group-data-[active=false]:shadow-none',
        'group-data-[active=false]:hover:shadow-none',
        'group-data-[active=false]:hover:bg-surface-variant/30',
      )}
      onClick={handleSelectProxy}
      {...props}
    >
      {/* 节点名称 */}
      <div className="flex items-center gap-2 px-2">
        <div className="truncate text-sm font-medium">{proxy.name}</div>
      </div>

      {/* 延迟测试按钮区域 */}
      <div className="flex items-center gap-2">
        <div className="flex-1" />

        <Button
          className={cn(
            'grid h-4 min-w-10 place-content-center px-2 text-center',
            delayColorClass(currentDelay),
          )}
          variant="raised"
          onClick={handleDelayClick}
          loading={delayTask.isPending}
          asChild
        >
          {/* 显示延迟值或闪电图标（未测试） */}
          {currentDelay > 0 ? (
            <span className="text-xs">{currentDelay} ms</span>
          ) : (
            <span>
              <FlashOnRounded className="py-1" />
            </span>
          )}
        </Button>
      </div>
    </Button>
  );
}
