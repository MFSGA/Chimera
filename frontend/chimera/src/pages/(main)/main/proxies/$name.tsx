/**
 * 代理组详情页（子路由）
 *
 * 迁移自 ref: `src/pages/(main)/main/proxies/group/$name.tsx`
 *（路由结构调整：ref 使用 `group/$name` 独立路由，本版本改为 `proxies/$name` 子路由）
 *
 * 职责：
 * - 作为 `/main/proxies` 的子路由，在 Sidebar 布局的右侧内容区显示
 * - 根据路由参数 `name` 查找并显示指定代理组的节点列表
 * - 使用 @tanstack/react-virtual 多列虚拟化网格布局（lanes）
 * - 提供延迟测试、节点选择、组流量显示等功能
 *
 * 当前阶段（Proxies Step 3 — 组详情迁移至 ref）：
 * - 替换 virtua 的 NodeList 为 @tanstack/react-virtual 多列网格（lanes）
 * - 替换 MUI DelayButton 为 ref 的 DelayTestButton（含加载动画、成功反馈）
 * - 添加 GroupHeader（组名 + 流量统计 + 滚动到当前节点）
 * - 添加 ProxyNodeButton（单节点选择 + 延迟测试）
 * - 使用 useContainerBreakpointValue 响应式调整列数（xs~xl）
 * - 使用 useCurrentGroupConnection 显示组流量（download/upload）
 */

import {
  useClashProxies,
  useProxyMode,
  type ClashProxiesQueryGroupItem,
} from '@chimera/interface';
import { useContainerBreakpointValue } from '@chimera/ui';
import { createFileRoute } from '@tanstack/react-router';
import { useVirtualizer } from '@tanstack/react-virtual';
import ArrowDownwardAltRounded from '~icons/material-symbols/arrow-downward-alt-rounded';
import ArrowUpwardAltRounded from '~icons/material-symbols/arrow-upward-alt-rounded';
import Radar from '~icons/material-symbols/radar';
import { filesize } from 'filesize';
import { useCallback, useMemo, useRef } from 'react';
import { Button } from '@/components/ui/button';
import { useScrollArea } from '@/components/ui/scroll-area';
import DelayTestButton from './_modules/delay-test-button';
import GroupHeader from './_modules/group-header';
import { useCurrentGroupConnection } from './_modules/hooks';
import ProxyNodeButton from './_modules/proxy-node-button';

/**
 * 注册 TanStack Router 文件路由
 * 此文件位于 `proxies/$name.tsx`，作为 `/(main)/main/proxies` 的子路由，
 * 路径模板: `/main/proxies/$name`
 *
 * 子路由会被渲染在 parent route.tsx 的 `<Outlet />` / `<AnimatedOutletPreset />` 中，
 * 因此组详情会显示在 Sidebar 的右侧内容区域。
 */
export const Route = createFileRoute('/(main)/main/proxies/$name')({
  component: RouteComponent,
});

/**
 * 代理组详情页组件
 *
 * 布局结构（匹配 ref）：
 * - GroupHeader：sticky 头部（组名 + 流量统计 + 滚动到当前节点按钮）
 * - Virtualized Grid：多列虚拟化网格（每个 ProxyNodeButton）
 * - DelayTestButton：浮动延迟测试按钮
 *
 */
function RouteComponent() {
  // 从路由参数中获取代理组名称
  const { name: proxyGroupName } = Route.useParams();

  const {
    proxies: { data: proxies },
  } = useClashProxies();

  const { value: proxyMode } = useProxyMode();

  // 查找当前选中的代理组（或 global 模式的 special 组）
  const currentGroup = useMemo<ClashProxiesQueryGroupItem | undefined>(() => {
    if (proxyMode.global) {
      return proxies?.global;
    }

    return proxies?.groups.find((group) => group.name === proxyGroupName);
  }, [proxies, proxyGroupName, proxyMode]);

  // 获取 AppContentScrollArea 的 viewportRef（用于同步虚拟滚动）
  const { viewportRef } = useScrollArea();

  // 响应式列数：根据容器宽度自动调整网格列数（匹配 ref）
  const lanes = useContainerBreakpointValue(
    viewportRef,
    {
      xs: 2,
      sm: 3,
      md: 4,
      lg: 5,
      xl: 6,
    },
    4,
  );

  // 初始化 @tanstack/react-virtual 多列虚拟化
  const virtualizer = useVirtualizer({
    count: currentGroup?.all?.length || 0,
    getScrollElement: () => viewportRef.current,
    estimateSize: () => 60,
    overscan: 5,
    lanes,
    measureElement: (element) => element?.getBoundingClientRect().height,
  });

  const virtualItems = virtualizer.getVirtualItems();

  // 滚动到当前选中节点（便于用户在列表中快速定位）
  const handleScrollToCurrentNode = useCallback(() => {
    const index = currentGroup?.all?.findIndex(
      (proxy) => proxy.name === currentGroup?.now,
    );

    if (index !== undefined) {
      virtualizer.scrollToIndex(index, {
        align: 'center',
        behavior: 'smooth',
      });
    }
  }, [currentGroup?.all, currentGroup?.now, virtualizer]);

  // 获取经过当前组的连接流量（用于显示 download/upload 速率）
  const currentGroupConnection = useCurrentGroupConnection(currentGroup);

  return (
    <>
      {/*
        GroupHeader：sticky 头部
        包含组名称、流量统计、滚动到当前节点按钮
      */}
      <GroupHeader>
        <div className="flex items-center gap-2">
          <div>{currentGroup?.name}</div>

          {/* 下载流量 */}
          <div className="flex items-center">
            <ArrowDownwardAltRounded className="size-6" />

            <span className="text-sm">
              {filesize(currentGroupConnection?.download ?? 0, {
                standard: 'iec',
              })}
              /s
            </span>
          </div>

          {/* 上传流量 */}
          <div className="flex items-center">
            <ArrowUpwardAltRounded className="size-6" />

            <span className="text-sm">
              {filesize(currentGroupConnection?.upload ?? 0, {
                standard: 'iec',
              })}
              /s
            </span>
          </div>
        </div>

        <div className="flex-1" />

        {/* 滚动到当前节点按钮 */}
        <Button icon className="size-8" onClick={handleScrollToCurrentNode}>
          <Radar className="size-4" />
        </Button>
      </GroupHeader>

      {/*
        虚拟化网格容器
        使用 @tanstack/react-virtual 的多列 lanes 布局
      */}
      <div
        className="relative m-2"
        data-slot="proxies-virtual-list"
        style={{
          width: 'calc(100% - 16px)',
          height: `${virtualizer.getTotalSize()}px`,
        }}
      >
        {virtualItems.map((virtualItem) => {
          const proxy = currentGroup?.all?.[virtualItem.index];

          if (!proxy) {
            return null;
          }

          return (
            <div
              key={virtualItem.index}
              ref={virtualizer.measureElement}
              className="group absolute top-0 left-0 p-1"
              style={{
                transform: `translateY(${virtualItem.start}px)`,
                width: `${100 / lanes}%`,
                left: `${virtualItem.lane * (100 / lanes)}%`,
              }}
              data-index={virtualItem.index}
              data-slot="proxies-virtual-item"
              data-active={String(proxy.name === currentGroup?.now)}
            >
              <ProxyNodeButton proxy={proxy} />
            </div>
          );
        })}
      </div>

      {/* 浮动延迟测试按钮 */}
      <DelayTestButton />
    </>
  );
}
