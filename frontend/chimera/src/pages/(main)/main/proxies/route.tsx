/**
 * 代理页面父路由
 *
 * 迁移自 ref: `src/pages/(main)/main/proxies/route.tsx`
 *
 * 职责：
 * - 提供双栏布局：左侧代理组列表 + 右侧代理节点详情
 * - 左侧 SidebarContent 显示 ProxiesNavigate（代理组导航）
 * - 右侧内容区域通过 AnimatedOutletPreset 渲染组详情
 * - 空状态：无代理时显示 Empty 提示（直连模式引导切换 / 空组引导添加订阅）
 * - 搜索功能：通过 validateSearch 解析 searchQuery 参数，传递给子路由
 *
 * 当前阶段（Step 4 — 添加搜索功能）：
 * - searchQuery 从 validateSearch 解析，绑定到 URL search 参数
 * - 内容区顶部显示搜索输入框，输入即搜索（实时更新 URL）
 * - 子路由 $name.tsx 通过 useSearch({ from: parentRouteId }) 读取 searchQuery
 * - 搜索框使用防抖处理（200ms）避免 URL 更新过于频繁
 *
 * 已完成阶段回顾：
 * - Step 1: 双栏布局 + 组导航 + AnimatedOutletPreset
 * - Step 2: searchQuery validateSearch + Empty + AppContentScrollArea + proxyMode
 * - Step 3: @tanstack/react-virtual 多列网格 + GroupHeader + ProxyNodeButton
 */

import { ProxyMode, useClashProxies, useProxyMode } from '@chimera/interface';
import { cn } from '@chimera/ui';
import { createFileRoute, Link, useNavigate } from '@tanstack/react-router';
import BoxOutlineRounded from '~icons/material-symbols/box-outline-rounded';
import DirectionsRunRounded from '~icons/material-symbols/directions-run-rounded';
import Search from '~icons/material-symbols/search';
import { useCallback, useRef, useState } from 'react';
import { AnimatedOutletPreset } from '@/components/router/animated-outlet';
import { Button } from '@/components/ui/button';
import { AppContentScrollArea } from '@/components/ui/scroll-area';
import { Sidebar, SidebarContent } from '@/components/ui/sidebar';
import { useLockFn } from '@/hooks/use-lock-fn';
import * as m from '@/paraglide/messages';
import ProxiesNavigate from './_modules/proxies-navigate';

/**
 * URL search 参数验证
 * 支持 `?searchQuery=keyword` 过滤代理组节点
 *
 * 匹配 ref 的 zodSearchValidator 但使用手动 validateSearch 维护（无 zod 依赖）
 */
export const Route = createFileRoute('/(main)/main/proxies')({
  component: RouteComponent,
  validateSearch: (search: Record<string, unknown>) => ({
    searchQuery:
      typeof search.searchQuery === 'string' ? search.searchQuery : undefined,
  }),
});

/**
 * 空状态组件
 *
 * 在以下时显示：
 * - 直连模式（Direct mode）：引导用户切换至 Rule/Global 模式
 * - 无代理组（空组）：引导用户添加订阅配置
 *
 * 匹配 ref 的 Empty 组件实现：
 * - 直连模式：显示 DirectionsRunRounded 图标 + 切换按钮（Rule / Global）
 * - 空组：显示 BoxOutlineRounded 图标 + "添加代理"按钮（跳转 Profiles 页面）
 */
const Empty = () => {
  const proxyMode = useProxyMode();

  const Icon = proxyMode.value.direct
    ? DirectionsRunRounded
    : BoxOutlineRounded;

  const handleSwitchMode = useLockFn(async (mode: ProxyMode) => {
    await proxyMode.upsert(mode);
  });

  return (
    <div
      className="absolute inset-0 flex flex-col items-center justify-center gap-4"
      data-slot="proxies-no-proxies"
    >
      <Icon className="text-surface-variant size-16" />

      <p
        className="text-surface-variant text-sm"
        data-slot="proxies-no-proxies-message"
      >
        {proxyMode.value.direct
          ? m.proxies_group_direct_message()
          : m.proxies_group_empty_message()}
      </p>

      {proxyMode.value.direct ? (
        <div className="flex items-center gap-2">
          <Button
            variant="raised"
            data-slot="switch-rule-mode-button"
            onClick={() => handleSwitchMode('rule')}
          >
            {m.proxies_group_direct_switch_rule_button_text()}
          </Button>

          <Button
            variant="raised"
            data-slot="switch-script-mode-button"
            onClick={() => handleSwitchMode('global')}
          >
            {m.proxies_group_direct_switch_global_button_text()}
          </Button>
        </div>
      ) : (
        <Button variant="raised" data-slot="proxies-no-proxies-button" asChild>
          <Link className="flex items-center gap-2" to="/main/profiles">
            {m.proxies_group_empty_button_text()}
          </Link>
        </Button>
      )}
    </div>
  );
};

/**
 * 代理页面布局组件
 *
 * 使用 Sidebar 组件提供双栏布局：
 * - 左侧：代理组导航列表（SidebarContent + ProxiesNavigate）
 * - 右侧：代理节点详情（AppContentScrollArea + 搜索框 + AnimatedOutletPreset）
 *
 * 搜索功能：
 * - 在右侧内容区顶部显示搜索输入框
 * - 输入即时更新 URL searchQuery 参数（通过 navigate）
 * - 子路由 $name.tsx 读取 searchQuery 过滤节点
 *
 * 条件显示：
 * - 直连模式：隐藏侧栏 + Empty 空状态
 * - 无代理组：隐藏侧栏 + Empty 空状态
 * - Rule/Script 模式且有组：侧栏 + 内容区（含搜索框）
 */
function RouteComponent() {
  const {
    proxies: { data: proxies },
  } = useClashProxies();

  // 检查无代理组的情况
  const isNoProxies = !proxies?.groups?.length || proxies?.groups?.length === 0;

  const proxyMode = useProxyMode();

  // 搜索功能
  const { searchQuery } = Route.useSearch();
  const navigate = useNavigate();
  // 防抖定时器引用，匹配 codebase 中 `as never` 的 search 参数风格
  const debounceRef = useRef<ReturnType<typeof setTimeout> | undefined>(
    undefined,
  );

  const handleSearchChange = useCallback(
    (value: string) => {
      // 防抖 200ms，避免每次输入都更新 URL
      if (debounceRef.current) {
        clearTimeout(debounceRef.current);
      }

      debounceRef.current = setTimeout(() => {
        navigate({
          search: (prev: { searchQuery?: string }) => ({
            ...prev,
            searchQuery: value || undefined,
          }),
          // TanStack Router 的 navigate search 类型较严格，
          // 使用 `as never` 匹配 codebase 模式（参见 scheme-provider.tsx:56）
        } as never);
      }, 200);
    },
    [navigate],
  );

  return (
    <Sidebar data-slot="proxies-container">
      {/* 左侧侧边栏：代理组列表（仅在 Rule/Script 模式且有组时显示） */}
      {!isNoProxies && (proxyMode.value.rule || proxyMode.value.script) && (
        <SidebarContent
          className="bg-surface-variant/10"
          data-slot="proxies-sidebar-scroll-area"
        >
          <ProxiesNavigate />
        </SidebarContent>
      )}

      {/* 
        右侧内容区域：代理节点详情
        使用 AppContentScrollArea 匹配 ref 的滚动管理方式
        空状态或直连模式时显示 Empty 组件
      */}
      <AppContentScrollArea
        className={cn(
          'group/proxies-content flex-[3_1_auto]',
          // 为 AnimatedOutletPreset 过渡动画保留 overflow-clip
          'overflow-clip',
        )}
        data-slot="proxies-content-scroll-area"
      >
        {isNoProxies || proxyMode.value.direct ? (
          <Empty />
        ) : (
          <div
            className={cn(
              'container mx-auto w-full min-w-full',
              'flex min-h-full flex-col',
            )}
            data-slot="proxies-content"
          >
            {/* 搜索输入框 — 仅在非空状态时显示 */}
            <div
              className="sticky top-0 z-10 flex items-center gap-2 p-3"
              data-slot="proxies-search-bar"
            >
              <div
                className={cn(
                  'flex h-10 w-full max-w-xs items-center gap-2 rounded-full px-4',
                  'bg-surface-variant/30',
                  'focus-within:ring-primary/30 focus-within:ring-2',
                )}
              >
                <Search className="text-on-surface-variant size-5 shrink-0" />

                <input
                  className={cn(
                    'h-full w-full bg-transparent text-sm outline-none',
                    'placeholder:text-on-surface-variant/50',
                  )}
                  placeholder={m.proxies_search_placeholder()}
                  defaultValue={searchQuery ?? ''}
                  onChange={(e) => handleSearchChange(e.target.value)}
                  data-slot="proxies-search-input"
                />
              </div>
            </div>

            <AnimatedOutletPreset className="flex flex-1 flex-col" />
          </div>
        )}
      </AppContentScrollArea>
    </Sidebar>
  );
}
