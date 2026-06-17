/**
 * 日志页面
 *
 * 迁移自 ref: `src/pages/(main)/main/logs/index.tsx`
 *
 * 职责：
 * - 显示 Clash 核心运行日志
 * - 支持日志级别过滤（通过 URL search params + 侧栏）
 * - 支持搜索过滤日志内容
 * - 使用 @tanstack/react-virtual 虚拟化大量日志条目
 * - 自动滚动到最新日志（当用户位于底部时）
 * - 右键清空日志
 * - 空状态展示
 *
 * 当前阶段（Logs Step 2 - 迁移至 ref 实现）：
 * - 使用 @tanstack/react-virtual 替代 virtua 实现虚拟滚动
 * - 使用 URL search params 驱动日志级别过滤（与 route.tsx 配合）
 * - 添加搜索过滤功能
 * - 添加空状态展示
 * - 添加自动滚动到底部行为
 * - 使用 LogLevelBadge 显示日志级别
 *
 * 后续迁移计划：
 * - 迁移到 ref 的 HighlightText 组件（搜索高亮）
 * - 添加右键菜单（RegisterContextMenu）
 * - 优化自动滚动逻辑
 */

import { useClashLogs } from '@chimera/interface';
import { cn } from '@chimera/ui';
import { DeleteSweepOutlined, InboxOutlined } from '@mui/icons-material';
import { IconButton, Tooltip } from '@mui/material';
import { createFileRoute } from '@tanstack/react-router';
import { useVirtualizer } from '@tanstack/react-virtual';
import { useLockFn } from 'ahooks';
import { useEffect, useMemo, useRef, useState, type RefObject } from 'react';
import * as m from '@/paraglide/messages';
import LogLevelBadge from './_modules/log-level-badge';
import { Route as LogsRoute } from './route';

export const Route = createFileRoute('/(main)/main/logs/')({
  component: RouteComponent,
});

/**
 * 日志内容查看器（Viewer）
 *
 * 负责：
 * - 根据 URL search params 的 level 过滤日志
 * - 根据搜索词过滤日志
 * - 使用 @tanstack/react-virtual 虚拟化渲染
 * - 当用户位于底部时自动滚动到最新日志
 */
function Viewer({
  search,
  scrollRef,
}: {
  search: string;
  scrollRef: RefObject<HTMLDivElement | null>;
}) {
  // 从父路由获取日志级别过滤条件
  const { level } = LogsRoute.useSearch();

  // 从 WebSocket 获取实时日志
  const {
    query: { data: logs },
  } = useClashLogs();

  /**
   * 过滤日志：
   * 1. 按日志级别过滤（来自 URL search params）
   * 2. 按搜索词过滤
   */
  const filteredLogs = useMemo(() => {
    if (!logs) {
      return [];
    }

    const levelFiltered = !level
      ? logs
      : logs.filter((log) => log.type.toLowerCase() === level);

    if (!search) {
      return levelFiltered;
    }

    const lowerSearch = search.toLowerCase();
    return levelFiltered.filter(
      (log) =>
        log.payload?.toLowerCase().includes(lowerSearch) ||
        log.type?.toLowerCase().includes(lowerSearch),
    );
  }, [logs, level, search]);

  // 初始化虚拟滚动
  const rowVirtualizer = useVirtualizer({
    count: filteredLogs.length,
    getScrollElement: () => scrollRef.current,
    estimateSize: () => 60,
    overscan: 5,
    measureElement: (element) => element?.getBoundingClientRect().height,
  });

  const virtualItems = rowVirtualizer.getVirtualItems();

  /**
   * 自动滚动到底部：
   * 当新日志到达且用户当前位于底部时，
   * 平滑滚动到最新日志位置
   */
  useEffect(() => {
    if (filteredLogs.length === 0) {
      return;
    }

    const scrollEl = scrollRef.current;
    if (!scrollEl) {
      return;
    }

    // 判断用户是否位于底部（允许 40px 误差）
    const isAtBottom =
      scrollEl.scrollHeight - scrollEl.scrollTop - scrollEl.clientHeight < 40;

    if (isAtBottom) {
      rowVirtualizer.scrollToIndex(filteredLogs.length - 1, {
        align: 'end',
        behavior: 'smooth',
      });
    }
  }, [filteredLogs, rowVirtualizer]);

  // 空状态
  if (filteredLogs.length === 0) {
    return (
      <div
        className="absolute inset-0 flex flex-col items-center justify-center gap-4"
        data-slot="logs-no-logs"
      >
        <InboxOutlined className="text-surface-variant !size-16" />

        <p
          className="text-surface-variant text-sm"
          data-slot="logs-no-logs-message"
        >
          {m.logs_empty_message()}
        </p>
      </div>
    );
  }

  return (
    <div
      className={cn(
        'relative mx-4 flex flex-col',
        'divide-outline-variant divide-y',
      )}
      data-slot="logs-virtual-list"
      style={{
        height: `${rowVirtualizer.getTotalSize()}px`,
      }}
    >
      {virtualItems.map((virtualItem) => {
        const log = filteredLogs[virtualItem.index];

        if (!log) {
          return null;
        }

        return (
          <div
            key={virtualItem.key}
            ref={rowVirtualizer.measureElement}
            data-index={virtualItem.index}
            data-slot="logs-virtual-item"
            className={cn(
              'absolute top-0 left-0 w-full select-text',
              'font-mono break-all',
              'flex flex-col py-2',
            )}
            style={{
              transform: `translateY(${virtualItem.start}px)`,
            }}
          >
            <div className="flex items-center gap-1">
              {/* 日志时间 */}
              <span className="text-surface-variant text-xs">
                {log.time || ''}
              </span>

              {/* 日志级别徽章 */}
              <LogLevelBadge>{log.type}</LogLevelBadge>
            </div>

            {/* 日志内容 */}
            <div className="text-sm font-normal text-wrap">
              {log.payload || ''}
            </div>
          </div>
        );
      })}
    </div>
  );
}

/**
 * 日志页面主组件
 *
 * 布局：
 * - 可滚动区域：Viewer（虚拟化日志列表）
 * - 底部工具栏：搜索框 + 清空日志按钮
 *
 * 与 ref 不同之处：
 * - 使用 Chimera 的 MUI 图标和 Tooltip
 * - 右键清空日志功能可通过工具栏按钮替代
 */
function RouteComponent() {
  const [search, setSearch] = useState('');
  const scrollRef = useRef<HTMLDivElement>(null);

  const {
    query: { data: logs },
    clean,
  } = useClashLogs();

  const handleClearLogs = useLockFn(async () => {
    await clean.mutateAsync();
  });

  return (
    <div
      className="divide-outline-variant flex min-h-0 flex-1 flex-col divide-y overflow-hidden"
      data-slot="logs-page-container"
    >
      {/* 
        日志内容区域 - 可滚动
        使用 ref 作为 scrollRef 供 @tanstack/react-virtual 使用
      */}
      <div
        ref={scrollRef}
        className="min-h-0 flex-1 overflow-y-auto"
        data-slot="logs-scroll-wrapper"
      >
        <Viewer search={search} scrollRef={scrollRef} />
      </div>

      {/* 
        底部工具栏：搜索框 + 清空日志按钮
      */}
      <div
        className="bg-mixed-background flex h-16 shrink-0 items-center gap-3 px-4"
        data-slot="logs-toolbar"
      >
        <input
          type="text"
          className={cn(
            'bg-surface-variant dark:bg-surface-variant/30',
            'h-10 min-w-0 flex-1 rounded-full px-4 text-sm outline-none',
          )}
          placeholder={m.logs_search_placeholder()}
          value={search}
          onChange={(e) => setSearch(e.target.value)}
        />

        <Tooltip title={m.logs_action_clear_log()}>
          <span>
            <IconButton
              size="small"
              color="inherit"
              onClick={handleClearLogs}
              disabled={!logs?.length}
            >
              <DeleteSweepOutlined />
            </IconButton>
          </span>
        </Tooltip>
      </div>
    </div>
  );
}
