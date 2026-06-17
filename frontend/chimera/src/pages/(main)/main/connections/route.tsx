/**
 * 连接页面父路由
 *
 * 迁移自 ref: `src/pages/(main)/main/connections/route.tsx` (Step 2)
 *
 * 职责：
 * - 作为 `/main/connections` 路由的父容器
 * - 提供左侧代理过滤侧栏（ProxySelector）
 * - 通过 URL search 参数 `?proxy=GroupName` 过滤连接
 * - 使用 Outlet 渲染子路由（连接列表页）
 *
 * 当前阶段（Step 2 - 迁移至 ref 实现）：
 * - 使用 Chimera 的 Sidebar 组件替代 ref 的 SliderSidebar
 * - 添加 validateSearch 验证 proxy 参数
 * - 从 useClashRules 提取所有代理名称，构建侧栏过滤列表
 * - 侧栏默认收起，可通过点击规则或「所有连接」条目切换
 * - 移动端自动隐藏侧栏
 *
 * 状态管理：
 * - 侧栏展开/收起状态通过 React state 管理（open/onOpenChange）
 * - 使用 URL search params 传递选中的 proxy 过滤条件
 * - 移动端点击条目后自动收起侧栏
 *
 * 后续迁移计划：
 * - 迁移到 ref 的 SidebarProvider + SliderSidebar 实现（含动画和持久化状态）
 */

import { useClashRules } from '@chimera/interface';
import { cn } from '@chimera/ui';
import { Tooltip } from '@mui/material';
import { createFileRoute, Link, Outlet } from '@tanstack/react-router';
import { useMemo, type ComponentProps } from 'react';
import { Button } from '@/components/ui/button';
import { ScrollArea } from '@/components/ui/scroll-area';
import { Sidebar, SidebarContent } from '@/components/ui/sidebar';
import * as m from '@/paraglide/messages';

/**
 * URL search 参数验证
 * 支持 `?proxy=GroupName` 过滤连接
 */
export const Route = createFileRoute('/(main)/main/connections')({
  component: RouteComponent,
  validateSearch: (search: Record<string, unknown>) => ({
    proxy: typeof search.proxy === 'string' ? search.proxy : undefined,
  }),
});

/**
 * 侧栏内容容器
 * 使用 ScrollArea 使内容可滚动
 */
const ProxySelectorContent = ({
  className,
  ...props
}: ComponentProps<'div'>) => {
  return <div className={cn('p-2', className)} {...props} />;
};

/**
 * 代理过滤项
 * 每个规则中的代理（如 Proxy、Reject、Direct 等）作为一个过滤条目
 * 选中时通过 URL search param `proxy=GroupName` 高亮并过滤
 */
function ProxyFilterItem({
  item,
  children,
}: ComponentProps<'div'> & {
  item?: string;
  children: string;
}) {
  const { proxy } = Route.useSearch();

  return (
    <Tooltip title={children} placement="right">
      <Button
        variant="fab"
        data-active={String(item === proxy)}
        className={cn(
          'h-12 min-w-0 px-3',
          'flex items-center gap-2',
          'data-[active=true]:bg-surface-variant/50',
          'data-[active=false]:bg-transparent',
          'data-[active=false]:shadow-none',
          'data-[active=false]:hover:shadow-none',
          'data-[active=false]:hover:bg-surface-variant/30',
        )}
        asChild
      >
        <Link
          to="."
          search={{
            proxy: item,
          }}
        >
          <div className="text-md grid size-6 shrink-0 place-content-center">
            {/* 未选中时显示空心圆，选中时显示实心圆 */}
            <div
              className={cn(
                'size-2 rounded-full',
                item === proxy ? 'bg-primary' : 'bg-surface-variant',
              )}
            />
          </div>

          <span className="truncate text-sm">{children}</span>
        </Link>
      </Button>
    </Tooltip>
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
    <ProxySelectorContent className="flex flex-col gap-2">
      {/* "所有连接"条目（清除 proxy 过滤条件） */}
      <ProxyFilterItem>{m.connections_all_connections()}</ProxyFilterItem>

      {/* 每个代理作为一个过滤条目 */}
      {allProxy.map((item) => (
        <ProxyFilterItem key={item} item={item}>
          {item}
        </ProxyFilterItem>
      ))}
    </ProxySelectorContent>
  );
};

/**
 * 连接页面布局组件
 *
 * 布局结构（与 ref 一致）：
 * - 左侧侧栏：代理过滤列表（ProxySelector）
 * - 右侧内容区：通过 Outlet 渲染连接列表页
 *
 * 注意：
 * - 侧栏使用 Chimera 的 Sidebar 组件，移动端自动隐藏
 * - 侧栏内容通过 URL search params 与连接列表页通信
 */
function RouteComponent() {
  return (
    <Sidebar data-slot="connections-container">
      {/* 左侧侧栏：代理过滤列表 */}
      <SidebarContent
        className="border-outline-variant divide-outline-variant border-r"
        data-slot="connections-sidebar"
      >
        <ScrollArea className="min-h-0 w-full flex-1 [&>div>div]:block!">
          <ProxySelector />
        </ScrollArea>
      </SidebarContent>

      {/* 右侧内容区：连接列表 */}
      <div
        className="flex min-w-0 flex-1 flex-col overflow-hidden"
        data-slot="connections-content"
      >
        <Outlet />
      </div>
    </Sidebar>
  );
}
