/**
 * 日志页面父路由
 *
 * 迁移自 ref: `src/pages/(main)/main/logs/route.tsx`
 *
 * 职责：
 * - 作为 `/main/logs` 路由的父容器
 * - 提供左侧日志级别过滤侧栏
 * - 通过 URL search 参数 `?level=error` 过滤日志
 * - 使用 Outlet 渲染子路由（日志列表页）
 *
 * 当前阶段（Logs Step 2 - 迁移至 ref 实现）：
 * - 添加 validateSearch 验证 level 参数（枚举：debug/info/warning/error）
 * - 使用 Chimera 的 Sidebar 组件替代 ref 的 SliderSidebar
 * - 侧栏显示所有日志级别按钮（All / Debug / Info / Warning / Error）
 * - 每个级别按钮使用 emoji 图标（与 ref 一致）
 * - 选中的级别在 URL search params 中反映
 */

import { Tooltip } from '@mui/material';
import { createFileRoute, Link, Outlet } from '@tanstack/react-router';
import { useMemo } from 'react';
import {
  Sidebar,
  SidebarContent,
} from '@/components/ui/sidebar';
import { Button } from '@/components/ui/button';
import { ScrollArea } from '@/components/ui/scroll-area';
import * as m from '@/paraglide/messages';
import { LogLevel } from './_modules/consts';

/**
 * 日志级别对应的 emoji 图标
 * 与 ref 一致：debug 🐛 / info ℹ️ / warning ⚠️ / error ❌ / all 📋
 */
const LogLevelIcon: Record<string, () => string> = {
  [LogLevel.Debug]: () => '🐛',
  [LogLevel.Info]: () => 'ℹ️',
  [LogLevel.Warning]: () => '⚠️',
  [LogLevel.Error]: () => '❌',
  all: () => '📋',
};

/**
 * URL search 参数验证
 * 支持 `?level=debug|info|warning|error` 过滤日志级别
 * 使用手动验证（而非 zod，因为项目未引入 zod）
 */
export const Route = createFileRoute('/(main)/main/logs')({
  component: RouteComponent,
  validateSearch: (search: Record<string, unknown>) => {
    const level = search.level;
    return {
      level: typeof level === 'string' &&
        Object.values(LogLevel).includes(level as LogLevel)
        ? (level as LogLevel)
        : undefined,
    };
  },
});

/**
 * 日志级别侧栏按钮
 * 点击后通过 URL search params 设置 level 过滤条件
 */
function LogLevelButton({
  level,
  children,
}: {
  /** 日志级别值，undefined 表示「全部」 */
  level?: LogLevel;
  children: string;
}) {
  const { level: currentLevel } = Route.useSearch();
  const Icon = level ? LogLevelIcon[level] : LogLevelIcon['all'];

  const isActive = level === currentLevel;

  return (
    <Tooltip title={children} placement="right">
      <Button
        variant="fab"
        data-active={String(isActive)}
        className={`
          h-12 min-w-0 px-3 flex items-center gap-2
          data-[active=true]:bg-surface-variant/50
          data-[active=false]:bg-transparent
          data-[active=false]:shadow-none
          data-[active=false]:hover:shadow-none
          data-[active=false]:hover:bg-surface-variant/30
        `}
        asChild
      >
        <Link
          to="."
          search={{
            level,
          }}
        >
          {/* 日志级别 emoji 图标 */}
          <div className="text-md grid size-6 shrink-0 place-content-center">
            <Icon />
          </div>

          {/* 级别名称 */}
          <span className="truncate text-sm capitalize">{children}</span>
        </Link>
      </Button>
    </Tooltip>
  );
}

/**
 * 日志页面布局组件
 *
 * 布局结构（与 ref 一致）：
 * - 左侧侧栏：日志级别过滤列表（All / Debug / Info / Warning / Error）
 * - 右侧内容区：通过 Outlet 渲染日志列表页
 */
function RouteComponent() {
  const logLevelEntries = Object.values(LogLevel);

  return (
    <Sidebar data-slot="logs-container">
      {/* 左侧侧栏：日志级别过滤 */}
      <SidebarContent
        className="border-r border-outline-variant"
        data-slot="logs-sidebar"
      >
        <ScrollArea className="min-h-0 w-full flex-1">
          <div className="flex flex-col gap-2 p-2">
            {/* "全部"条目（清除 level 过滤条件） */}
            <LogLevelButton>{'All'}</LogLevelButton>

            {/* 每个日志级别作为一个过滤条目 */}
            {logLevelEntries.map((level) => (
              <LogLevelButton key={level} level={level}>
                {level}
              </LogLevelButton>
            ))}
          </div>
        </ScrollArea>
      </SidebarContent>

      {/* 右侧内容区：日志列表 */}
      <div
        className="flex min-w-0 flex-1 flex-col overflow-hidden"
        data-slot="logs-content"
      >
        <Outlet />
      </div>
    </Sidebar>
  );
}
