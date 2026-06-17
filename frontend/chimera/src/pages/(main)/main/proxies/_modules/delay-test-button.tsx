/**
 * 代理组延迟测试浮动按钮
 *
 * 迁移自 ref: `src/pages/(main)/main/proxies/group/_modules/delay-test-button.tsx`
 *
 * 职责：
 * - 浮动在内容区右下角的 FAB，点击触发当前组的全部延迟测试
 * - 使用 useBlockTask 管理加载/成功状态
 * - 测试完成后显示 1 秒绿色成功效果
 * - 使用 Tooltip 显示按钮功能说明
 *
 * 当前阶段（Proxies Step 3 — 组详情迁移）：
 * - 匹配 ref 的 DelayTestButton 设计（含加载动画、成功状态、毛玻璃效果）
 * - 使用 MUI Tooltip 替代 ref 的自定义 Tooltip 组件（Chimera 生态）
 * - 使用 unplugin-icons 导入 BoltRounded 图标
 *
 * 后续计划：
 * - 配合 useScrollArea 的 content-bottom 检测，动态调整按钮位置
 */

import { useClashProxies } from '@chimera/interface';
import { cn } from '@chimera/ui';
import { Tooltip } from '@mui/material';
import BoltRounded from '~icons/material-symbols/bolt-rounded';
import { useState } from 'react';
import { useBlockTask } from '@/components/providers/block-task-provider';
import { Button } from '@/components/ui/button';
import { useLockFn } from '@/hooks/use-lock-fn';
import * as m from '@/paraglide/messages';
import { Route as NameRoute } from '../$name';

/**
 * 延迟测试浮动按钮
 *
 * 使用 updateGroupDelay mutation 触发全部节点延迟测试，
 * 完成后通过短暂的成功状态（isSuccess）反馈给用户。
 */
export default function DelayTestButton() {
  const { name } = NameRoute.useParams();

  const { updateGroupDelay } = useClashProxies();

  const [isSuccess, setIsSuccess] = useState(false);

  const blockTask = useBlockTask(`delay-group-test-${name}`, async () => {
    await updateGroupDelay.mutateAsync([name]);
  });

  const handleClick = useLockFn(async () => {
    await blockTask.execute();

    // 成功效果：显示 1 秒绿色反馈
    setIsSuccess(true);
    await new Promise((resolve) => setTimeout(resolve, 1000));
    setIsSuccess(false);
  });

  return (
    <div
      data-slot="delay-test-button"
      data-success={String(isSuccess)}
      data-loading={String(blockTask.isPending)}
      className={cn(
        'absolute right-4 ml-auto w-fit',
        // 按钮位置基于视口高度和其他组件高度计算（匹配 ref）
        'top-[calc(100vh-40px-64px-72px)]',
        'sm:top-[calc(100vh-40px-48px-72px)]',
        'transition-[top] duration-500 sm:bottom-4',
        'data-[loading=false]:data-[success=false]:group-data-[bottom=true]/proxies-content:top-full',
      )}
    >
      <Tooltip
        title={
          blockTask.isPending
            ? m.proxies_group_delay_test_pending_title()
            : m.proxies_group_delay_test_title()
        }
      >
        <Button
          data-slot="delay-test-button-trigger"
          data-success={String(isSuccess)}
          data-loading={String(blockTask.isPending)}
          className={cn(
            "**:data-[slot='circular-progress']:size-6",
            'transition-colors',
            'backdrop-blur',
            'data-[loading=false]:bg-primary-container/35',
            'data-[loading=false]:dark:bg-on-primary/35',
            'data-[success=true]:bg-green-500/30',
            'data-[success=true]:dark:bg-green-700/50',
          )}
          variant="fab"
          icon
          loading={blockTask.isPending}
          onClick={handleClick}
        >
          <BoltRounded className="size-6" />
        </Button>
      </Tooltip>
    </div>
  );
}
