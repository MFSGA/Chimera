/**
 * 代理组头部组件
 *
 * 迁移自 ref: `src/pages/(main)/main/proxies/group/_modules/group-header.tsx`
 *
 * 职责：
 * - 固定在内容区顶部（sticky），显示当前代理组信息
 * - 移动端显示返回按钮（链接到 /main/proxies）
 * - 子元素插槽显示组名称、流量统计、滚动到当前节点按钮等
 *
 * 当前阶段（Proxies Step 3 — 组详情迁移）：
 * - 匹配 ref 的 sticky 头部设计
 * - 使用 group-data-[scroll-direction] 切换 padding（配合后续滚动方向检测）
 * - BackButton 仅在移动端渲染（md:hidden）
 *
 * 后续计划：
 * - 配合 useScrollArea 的滚动方向检测，动态调整 padding
 */

import { cn } from '@chimera/ui';
import { Link } from '@tanstack/react-router';
import ArrowBackIosNewRounded from '~icons/material-symbols/arrow-back-ios-new-rounded';
import type { ComponentProps } from 'react';
import { Button } from '@/components/ui/button';

/**
 * 返回按钮（仅移动端显示）
 */
const BackButton = () => {
  return (
    <Button icon className="flex items-center justify-center md:hidden" asChild>
      <Link to="/main/proxies" search={{ searchQuery: undefined }}>
        <ArrowBackIosNewRounded className="size-4" />
      </Link>
    </Button>
  );
};

/**
 * 代理组头部容器
 *
 * 使用 sticky 定位固定在内容区顶部，
 * children 插槽中通常包含：
 * - 组名称
 * - 下载/上传流量速率
 * - 滚动到当前节点按钮
 */
export default function GroupHeader({
  children,
  className,
  ...props
}: ComponentProps<'div'>) {
  return (
    <div
      className={cn(
        'sticky top-0 z-10 transition-[padding] duration-500',
        'bg-mixed-background',
        'flex items-center gap-1',
        'py-2 pr-4 pl-2 md:py-4 md:pl-4',
        // 配合 proxies-content 的滚动方向切换 padding（匹配 ref）
        'group-data-[scroll-direction=down]/proxies-content:pr-6',
        'group-data-[scroll-direction=down]/proxies-content:pl-3',
        'group-data-[scroll-direction=down]/proxies-content:md:pl-6',
        className,
      )}
      {...props}
    >
      <BackButton />

      {children}
    </div>
  );
}
