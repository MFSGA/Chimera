/**
 * 规则页面
 *
 * 迁移自 ref: `src/pages/(main)/main/rules/index.tsx`
 *
 * 职责：
 * - 显示所有 Clash 规则
 * - 支持按关键字过滤规则（类型、负载、代理）
 * - 支持按代理过滤规则（通过 URL search params + 侧栏）
 * - 使用 @tanstack/react-table 渲染表格
 * - 使用 @tanstack/react-virtual 虚拟化大量规则
 *
 * 当前阶段（Rules Step 2 - 迁移至 ref 实现）：
 * - 使用 @tanstack/react-table + @tanstack/react-virtual 替代 virtua
 * - 使用 URL search params 驱动代理过滤（与 route.tsx 配合）
 * - 搜索过滤规则（类型、负载、代理）
 * - 表格列：Index / Type / Payload / Proxy
 * - 使用 useScrollArea 获取 viewportRef 供 Virtualizer 使用
 *
 * 后续迁移计划：
 * - 添加列排序功能
 * - 添加 HighlightText 搜索高亮组件
 */

import { useClashRules } from '@chimera/interface';
import { cn } from '@chimera/ui';
import { createFileRoute } from '@tanstack/react-router';
import {
  flexRender,
  getCoreRowModel,
  getSortedRowModel,
  useReactTable,
} from '@tanstack/react-table';
import { useVirtualizer } from '@tanstack/react-virtual';
import { useMemo, useRef, useState } from 'react';
import * as m from '@/paraglide/messages';
import { Route as RulesRoute } from './route';

export const Route = createFileRoute('/(main)/main/rules/')({
  component: RouteComponent,
});

/**
 * 规则内容查看器（Viewer）
 *
 * 负责：
 * - 从 URL search params 获取 proxy 过滤条件
 * - 从搜索框获取搜索关键词
 * - 使用 @tanstack/react-table 渲染规则表格
 * - 使用 @tanstack/react-virtual 虚拟化大量行
 */
function Viewer({ search }: { search: string }) {
  // 从父路由获取 proxy 过滤条件
  const { proxy } = RulesRoute.useSearch();

  // 滚动容器 ref（供 Virtualizer 使用）
  const scrollRef = useRef<HTMLDivElement>(null);

  // 获取规则数据
  const { data } = useClashRules();

  /**
   * 过滤规则：
   * 1. 按代理过滤（来自 URL search params）
   * 2. 按搜索关键词过滤（类型、负载、代理）
   */
  const filteredRules = useMemo(() => {
    const rules = data?.rules ?? [];

    // 按代理过滤
    const proxyFilteredRules = proxy
      ? rules.filter((rule) => rule.proxy === proxy)
      : rules;

    // 按搜索关键词过滤
    if (!search.trim()) {
      return proxyFilteredRules;
    }

    const searchLower = search.toLowerCase();

    return proxyFilteredRules.filter((rule) => {
      return (
        rule.type?.toLowerCase().includes(searchLower) ||
        rule.payload?.toLowerCase().includes(searchLower) ||
        rule.proxy?.toLowerCase().includes(searchLower)
      );
    });
  }, [data?.rules, proxy, search]);

  // 初始化 @tanstack/react-table
  const table = useReactTable({
    data: filteredRules,
    columns: [
      {
        accessorKey: 'Index',
        header: 'Index',
        cell: (info) => info.row.index + 1,
        size: 80,
      },
      {
        accessorKey: 'type',
        header: 'Type',
        cell: (info) => (
          <span className="text-sm">{info.row.original.type || ''}</span>
        ),
        size: 160,
      },
      {
        accessorKey: 'payload',
        header: 'Payload',
        cell: (info) => (
          <span className="font-mono text-sm break-all">
            {info.row.original.payload || ''}
          </span>
        ),
        size: 400,
      },
      {
        accessorKey: 'proxy',
        header: 'Proxy',
        cell: (info) => (
          <span className="text-sm">{info.row.original.proxy || ''}</span>
        ),
        size: 160,
      },
    ],
    getCoreRowModel: getCoreRowModel(),
    getSortedRowModel: getSortedRowModel(),
  });

  const { rows } = table.getRowModel();

  // 初始化 @tanstack/react-virtual 虚拟滚动
  const rowVirtualizer = useVirtualizer({
    count: rows.length,
    getScrollElement: () => scrollRef.current,
    estimateSize: () => 48,
    overscan: 10,
    measureElement: (element) => element?.getBoundingClientRect().height,
  });

  const virtualItems = rowVirtualizer.getVirtualItems();

  return (
    <div
      ref={scrollRef}
      className="min-h-0 flex-1 overflow-y-auto"
      data-slot="rules-scroll-area"
    >
      <div
        className="mx-auto max-w-7xl px-8"
        data-slot="rules-virtual-container"
        style={{ height: `${rowVirtualizer.getTotalSize()}px` }}
      >
        <table
          className="w-full min-w-208 table-fixed"
          data-slot="rules-virtual-table"
        >
          {/* 列宽定义 */}
          <colgroup>
            <col className="w-20" />
            <col className="w-40" />
            <col />
            <col className="w-40" />
          </colgroup>

          <tbody className="select-text" data-slot="rules-virtual-tbody">
            {virtualItems.map((virtualRow, index) => {
              const row = rows[virtualRow.index];

              if (!row) {
                return null;
              }

              const offset = virtualRow.start - index * virtualRow.size;

              return (
                <tr
                  key={row.id}
                  data-index={virtualRow.index}
                  ref={rowVirtualizer.measureElement}
                  data-slot="rules-virtual-tr"
                  className={cn(
                    'hover:bg-primary/5',
                    'border-outline-variant/30 border-b',
                  )}
                  style={{
                    height: `${virtualRow.size}px`,
                    transform: `translateY(${offset}px)`,
                  }}
                >
                  {row.getVisibleCells().map(({ column, id, getContext }) => (
                    <td
                      key={id}
                      data-slot="rules-virtual-td"
                      className="truncate px-3 py-2 text-sm"
                    >
                      {flexRender(column.columnDef.cell, getContext())}
                    </td>
                  ))}
                </tr>
              );
            })}
          </tbody>
        </table>
      </div>
    </div>
  );
}

/**
 * 规则页面主组件
 *
 * 布局：
 * - 可滚动区域：Viewer（虚拟化规则表格）
 * - 底部工具栏：搜索框
 */
function RouteComponent() {
  const [search, setSearch] = useState('');

  return (
    <div
      className="divide-outline-variant flex min-h-0 flex-1 flex-col divide-y overflow-hidden"
      data-slot="rules-page-container"
    >
      {/* 规则表格内容区域 */}
      <Viewer search={search} />

      {/* 
        底部工具栏：搜索框
        支持按类型、负载、代理名称过滤
      */}
      <div
        className="bg-mixed-background flex h-16 shrink-0 items-center px-4"
        data-slot="rules-search"
      >
        <input
          type="text"
          className={cn(
            'bg-surface-variant dark:bg-surface-variant/30',
            'h-10 w-full rounded-full px-4 pr-10 text-sm outline-none',
          )}
          data-slot="rules-search-input-field"
          placeholder={m.rules_list_all_proxies()}
          value={search}
          onChange={(e) => setSearch(e.target.value)}
        />
      </div>
    </div>
  );
}
