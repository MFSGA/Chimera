/**
 * 规则页面父路由
 *
 * 迁移自 ref: `src/pages/(main)/main/rules/route.tsx`
 *
 * 职责：
 * - 作为 `/main/rules` 路由的父容器
 * - 提供左侧代理过滤侧栏（ProxySelector）
 * - 通过 URL search 参数 `?proxy=GroupName` 过滤规则
 * - 使用 Outlet 渲染子路由（规则列表页）
 *
 * 当前阶段（Rules Step 2 - 迁移至 ref 实现）：
 * - 添加 validateSearch 验证 proxy 参数
 * - 使用 Chimera 的 Sidebar 组件替代 ref 的 SliderSidebar
 * - 侧栏显示所有去重的代理名称（从规则数据中提取）
 * - 使用 ProxyIcon 显示每个代理的图标
 * - 选中的代理通过 URL search params 反映
 */

import { useClashRules } from '@chimera/interface';
import { cn } from '@chimera/ui';
import { Tooltip } from '@mui/material';
import { createFileRoute, Link, Outlet } from '@tanstack/react-router';
import { useMemo } from 'react';
import { Button } from '@/components/ui/button';
import { ScrollArea } from '@/components/ui/scroll-area';
import { Sidebar, SidebarContent } from '@/components/ui/sidebar';
import * as m from '@/paraglide/messages';
import ProxyIcon from './_modules/proxy-icon';

/**
 * URL search 参数验证
 * 支持 `?proxy=GroupName` 过滤规则
 */
export const Route = createFileRoute('/(main)/main/rules')({
  component: RouteComponent,
  validateSearch: (search: Record<string, unknown>) => ({
    proxy: typeof search.proxy === 'string' ? search.proxy : undefined,
  }),
});

/**
 * 代理过滤项
 * 每个规则中的代理（如 Proxy、Reject、Direct 等）作为一个过滤条目
 * 选中时通过 URL search param `proxy=GroupName` 高亮并过滤
 */
function ProxyFilterItem({
  item,
  children,
}: {
  item?: string;
  children: string;
}) {
  const { proxy } = Route.useSearch();

  const isActive = item === proxy;

  return (
    <Tooltip title={children} placement="right">
      <Button
        variant="fab"
        data-active={String(isActive)}
        className={`data-[active=true]:bg-surface-variant/50 data-[active=false]:hover:bg-surface-variant/30 flex h-12 min-w-0 items-center gap-2 px-3 data-[active=false]:bg-transparent data-[active=false]:shadow-none data-[active=false]:hover:shadow-none`}
        asChild
      >
        <Link
          to="."
          search={{
            proxy: item,
          }}
        >
          {/* 代理图标 */}
          <div className="text-md grid size-6 shrink-0 place-content-center">
            {item ? <ProxyIcon groupName={item} /> : <ListIcon />}
          </div>

          {/* 代理名称 */}
          <span className="truncate text-sm">{children}</span>
        </Link>
      </Button>
    </Tooltip>
  );
}

/**
 * 列表图标（用于「所有代理」条目）
 * 使用 SVG 替代 ref 的 ListRounded 图标
 */
function ListIcon() {
  return (
    <svg
      className="size-5"
      viewBox="0 0 24 24"
      fill="none"
      stroke="currentColor"
      strokeWidth="2"
    >
      <path d="M3 6h18M3 12h18M3 18h18" />
    </svg>
  );
}

/**
 * 代理过滤器组件
 *
 * 从 useClashRules 获取所有规则，
 * 提取每个规则中的 proxy 字段（去重），
 * 构建侧栏过滤列表。
 */
const ProxySelector = () => {
  const { data } = useClashRules();

  // 从规则中提取所有去重的代理名称
  const allProxy = useMemo(() => {
    const proxies =
      data?.rules
        .map((rule) => rule.proxy)
        .filter((proxy): proxy is string => !!proxy) ?? [];

    return [...new Set(proxies)];
  }, [data]);

  return (
    <div className="flex flex-col gap-2 p-2">
      {/* "所有代理"条目（清除 proxy 过滤条件） */}
      <ProxyFilterItem>{m.rules_list_all_proxies()}</ProxyFilterItem>

      {/* 每个代理作为一个过滤条目 */}
      {allProxy.map((item) => (
        <ProxyFilterItem key={item} item={item}>
          {item}
        </ProxyFilterItem>
      ))}
    </div>
  );
};

/**
 * 规则页面布局组件
 *
 * 布局结构（与 ref 一致）：
 * - 左侧侧栏：代理过滤列表（ProxySelector）
 * - 右侧内容区：通过 Outlet 渲染规则列表页
 */
function RouteComponent() {
  return (
    <Sidebar data-slot="rules-container">
      {/* 左侧侧栏：代理过滤列表 */}
      <SidebarContent
        className="border-outline-variant border-r"
        data-slot="rules-sidebar"
      >
        <ScrollArea className="min-h-0 w-full flex-1 [&>div>div]:block!">
          <ProxySelector />
        </ScrollArea>
      </SidebarContent>

      {/* 右侧内容区：规则列表 */}
      <div
        className="flex min-w-0 flex-1 flex-col overflow-hidden"
        data-slot="rules-content"
      >
        <Outlet />
      </div>
    </Sidebar>
  );
}
