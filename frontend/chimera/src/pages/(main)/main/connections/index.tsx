/**
 * 连接页面
 *
 * 迁移自 ref: `src/pages/(main)/main/connections/index.tsx` (Step 2)
 *
 * 职责：
 * - 显示所有活跃的网络连接列表
 * - 支持搜索过滤连接（主机、链、规则等）
 * - 支持关闭全部连接（右键菜单/工具栏按钮）
 * - 使用 @tanstack/react-table + @tanstack/react-virtual 虚拟化表格
 * - 列宽可拖拽调整，状态持久化到 localStorage
 * - 支持列排序
 * - 空状态展示
 *
 * 迁移步骤（Step 2 - 迁移至 ref 实现）：
 * - 使用 @tanstack/react-table 替代 Material React Table
 * - 使用 @tanstack/react-virtual 实现虚拟滚动
 * - 列宽调整持久化（localStorage）
 * - 添加上下行速度计算（基于前后两次快照差值）
 * - 添加空状态展示
 * - 工具栏：搜索 + 关闭全部连接按钮
 * - 右键上下文菜单：查看详情 / 关闭连接（在 table-row.tsx 中实现）
 *
 * 后续迁移计划：
 * - 迁移连接详情对话框（table-row.tsx 已实现基础版）
 */

import {
  useClashConnections,
  type ClashConnectionItem,
} from '@chimera/interface';
import { cn } from '@chimera/ui';
import { CloseRounded, InboxOutlined } from '@mui/icons-material';
import { IconButton, Tooltip } from '@mui/material';
import { createFileRoute } from '@tanstack/react-router';
import {
  ColumnDef,
  ColumnSizingState,
  flexRender,
  getCoreRowModel,
  getSortedRowModel,
  useReactTable,
  type Updater,
} from '@tanstack/react-table';
import { useVirtualizer } from '@tanstack/react-virtual';
import { useLocalStorage } from '@uidotdev/usehooks';
import { useLockFn } from 'ahooks';
import dayjs from 'dayjs';
import relativeTime from 'dayjs/plugin/relativeTime';
import {
  useCallback,
  useEffect,
  useMemo,
  useState,
  type ReactNode,
} from 'react';
import { Button } from '@/components/ui/button';
import {
  AppContentScrollArea,
  useScrollArea,
} from '@/components/ui/scroll-area';
import * as m from '@/paraglide/messages';
import { containsSearchTerm } from '@/utils';
import parseTraffic from '@/utils/parse-traffic';
import TableRow from './_modules/table-row';
import { Route as ConnectionsRoute } from './route';

// 启用 dayjs relativeTime 插件
dayjs.extend(relativeTime);

/**
 * 连接行数据类型
 * 在 ClashConnectionItem 基础上扩展了速度计算字段
 */
export type ConnectionRow = ClashConnectionItem & {
  closed: boolean;
  downloadSpeed: number;
  uploadSpeed: number;
};

/**
 * 列宽配置 localStorage 键名
 * 与 ref 保持一致：'connections-column-sizing-v2'
 */
const COLUMN_SIZING_STORAGE_KEY = 'connections-column-sizing-v2';

export const Route = createFileRoute('/(main)/main/connections/')({
  component: RouteComponent,
});

/**
 * 高亮搜索匹配文本的组件
 * 将文本按搜索词分割，匹配部分加亮色背景
 */
function HighlightText({
  text,
  search,
}: {
  text: string;
  search: string;
}): ReactNode {
  if (!search || !text) {
    return text;
  }

  const lowerText = text.toLowerCase();
  const lowerSearch = search.toLowerCase();
  const idx = lowerText.indexOf(lowerSearch);

  if (idx === -1) {
    return text;
  }

  return (
    <>
      {text.slice(0, idx)}
      <mark className="bg-primary/20 dark:bg-primary/40 rounded px-0.5">
        {text.slice(idx, idx + search.length)}
      </mark>
      {text.slice(idx + search.length)}
    </>
  );
}

/**
 * 连接表格查看器（Viewer）
 *
 * 负责：
 * - 从 WebSocket 拉取连接快照数据
 * - 计算上下行速度
 * - 按搜索词和 proxy 参数过滤
 * - 使用 @tanstack/react-table 渲染表格
 * - 使用 @tanstack/react-virtual 虚拟化大量行
 * - 列宽调整通过 ResizeObserver + localStorage 持久化
 */
function Viewer({ search }: { search: string }) {
  // 从 URL search 参数获取 proxy 过滤条件
  const { proxy } = ConnectionsRoute.useSearch();

  // 列宽状态（持久化到 localStorage）
  const [columnSizing, setColumnSizing] = useLocalStorage<ColumnSizingState>(
    COLUMN_SIZING_STORAGE_KEY,
    {},
  );

  // WebSocket 连接数据
  const { data: clashConnections } = useClashConnections();

  // 获取 ScrollArea 的 viewportRef（与 AnimatedOutletPreset 配合）
  const { viewportRef } = useScrollArea();

  /**
   * 处理列宽变更回调
   * 使用 useCallback 避免不必要的重渲染
   */
  const handleColumnSizingChange = useCallback(
    (updater: Updater<ColumnSizingState>) => {
      setColumnSizing((prev) => {
        return typeof updater === 'function' ? updater(prev) : updater;
      });
    },
    // oxlint-disable-next-line eslint-plugin-react-hooks/exhaustive-deps
    [],
  );

  /**
   * 计算连接数据（速度、过滤）
   *
   * 与 ref 一致：
   * 1. 取最新的两个快照
   * 2. 按 proxy 过滤（chains 包含指定代理组）
   * 3. 计算上下行速度（当前 - 前一次）
   * 4. 按搜索词过滤
   */
  const data = useMemo<ConnectionRow[]>(() => {
    const allSnapshots = clashConnections ?? [];

    const latestConnections = allSnapshots.at(-1)?.connections ?? [];
    const prevConnections = allSnapshots.at(-2)?.connections ?? [];

    const prevMap = new Map(prevConnections.map((c) => [c.id, c]));

    const all = latestConnections
      .filter((conn) => (proxy ? conn.chains?.includes(proxy) : true))
      .map((conn) => {
        const prev = prevMap.get(conn.id);
        return {
          ...conn,
          closed: false,
          downloadSpeed: prev ? conn.download - prev.download : 0,
          uploadSpeed: prev ? conn.upload - prev.upload : 0,
        };
      })
      .filter((c) => (search ? containsSearchTerm(c, search) : true));

    return all;
  }, [clashConnections, search, proxy]);

  /**
   * 表格列定义
   * 与 ref 一致：Host / Chains / Downloaded / Uploaded / DL Speed / UL Speed / Process / Rule / Time / Source / Destination IP / Type
   */
  const columns = useMemo(
    () =>
      [
        {
          header: 'Host',
          accessorFn: ({ metadata }) => metadata.host || metadata.destinationIP,
          size: 320,
          cell: (info) => (
            <HighlightText
              text={
                info.row.original.metadata.host ||
                info.row.original.metadata.destinationIP ||
                ''
              }
              search={search}
            />
          ),
        },
        {
          header: 'Chains',
          accessorFn: ({ chains }) => [...chains].reverse().join(' / '),
          size: 360,
          cell: (info) => (
            <HighlightText
              text={[...info.row.original.chains].reverse().join(' / ') || ''}
              search={search}
            />
          ),
        },
        {
          header: 'Downloaded',
          accessorFn: ({ download }) => parseTraffic(download).join(' '),
          sortingFn: (rowA, rowB) =>
            rowA.original.download - rowB.original.download,
          size: 120,
          cell: (info) => (
            <HighlightText
              text={parseTraffic(info.row.original.download).join(' ')}
              search={search}
            />
          ),
        },
        {
          header: 'Uploaded',
          accessorFn: ({ upload }) => parseTraffic(upload).join(' '),
          sortingFn: (rowA, rowB) =>
            rowA.original.upload - rowB.original.upload,
          size: 120,
          cell: (info) => (
            <span>{parseTraffic(info.row.original.upload).join(' ')}</span>
          ),
        },
        {
          header: 'DL Speed',
          accessorFn: ({ downloadSpeed }) =>
            parseTraffic(downloadSpeed).join(' ') + '/s',
          sortingFn: (rowA, rowB) =>
            rowA.original.downloadSpeed - rowB.original.downloadSpeed,
          size: 120,
          cell: (info) => (
            <span>
              {parseTraffic(info.row.original.downloadSpeed).join(' ')}/s
            </span>
          ),
        },
        {
          header: 'UL Speed',
          accessorFn: ({ uploadSpeed }) =>
            parseTraffic(uploadSpeed).join(' ') + '/s',
          sortingFn: (rowA, rowB) =>
            rowA.original.uploadSpeed - rowB.original.uploadSpeed,
          size: 120,
          cell: (info) => (
            <span>
              {parseTraffic(info.row.original.uploadSpeed).join(' ')}/s
            </span>
          ),
        },
        {
          header: 'Process',
          accessorFn: ({ metadata }) => metadata.process,
          size: 160,
          cell: (info) => (
            <HighlightText
              text={info.row.original.metadata.process || ''}
              search={search}
            />
          ),
        },
        {
          header: 'Rule',
          accessorFn: ({ rule, rulePayload }) =>
            rulePayload ? `${rule} (${rulePayload})` : rule,
          size: 200,
          cell: (info) => (
            <HighlightText
              text={
                info.row.original.rulePayload
                  ? `${info.row.original.rule} (${info.row.original.rulePayload})`
                  : info.row.original.rule || ''
              }
              search={search}
            />
          ),
        },
        {
          header: 'Time',
          accessorFn: ({ start }) => dayjs(start).fromNow(),
          sortingFn: (rowA, rowB) =>
            dayjs(rowA.original.start).diff(rowB.original.start),
          size: 120,
          cell: (info) => (
            <span
              title={dayjs(info.row.original.start).format(
                'YYYY-MM-DD HH:mm:ss',
              )}
            >
              {dayjs(info.row.original.start).fromNow()}
            </span>
          ),
        },
        {
          header: 'Source',
          accessorFn: ({ metadata: { sourceIP, sourcePort } }) =>
            `${sourceIP}:${sourcePort}`,
          size: 160,
          cell: (info) => (
            <HighlightText
              text={`${info.row.original.metadata.sourceIP}:${info.row.original.metadata.sourcePort}`}
              search={search}
            />
          ),
        },
        {
          header: 'Destination IP',
          accessorFn: ({ metadata: { destinationIP, destinationPort } }) =>
            `${destinationIP}:${destinationPort}`,
          size: 160,
          cell: (info) => (
            <HighlightText
              text={`${info.row.original.metadata.destinationIP || ''}:${info.row.original.metadata.destinationPort || ''}`}
              search={search}
            />
          ),
        },
        {
          header: 'Type',
          accessorFn: ({ metadata }) =>
            `${metadata.type} (${metadata.network})`,
          size: 120,
          cell: (info) => (
            <HighlightText
              text={`${info.row.original.metadata.type} (${info.row.original.metadata.network})`}
              search={search}
            />
          ),
        },
      ] satisfies Array<ColumnDef<ConnectionRow>>,
    [search],
  );

  // 初始化 @tanstack/react-table
  const table = useReactTable({
    data,
    columns,
    state: {
      columnSizing,
    },
    onColumnSizingChange: handleColumnSizingChange,
    getCoreRowModel: getCoreRowModel(),
    getSortedRowModel: getSortedRowModel(),
    enableColumnResizing: true,
    columnResizeMode: 'onChange',
  });

  const { rows } = table.getRowModel();

  // 初始化 @tanstack/react-virtual 虚拟滚动
  const rowVirtualizer = useVirtualizer({
    count: rows.length,
    getScrollElement: () => viewportRef.current,
    estimateSize: () => 40,
    overscan: 10,
    measureElement: (element) => element?.getBoundingClientRect().height,
  });

  const virtualItems = rowVirtualizer.getVirtualItems();

  // 视口宽度监听（用于列宽自适应）
  const [viewportWidth, setViewportWidth] = useState(0);

  useEffect(() => {
    const viewport = viewportRef.current;

    if (!viewport) {
      return;
    }

    const updateWidth = () => {
      setViewportWidth(viewport.clientWidth);
    };

    updateWidth();

    const observer = new ResizeObserver(updateWidth);
    observer.observe(viewport);

    return () => {
      observer.disconnect();
    };
  }, [viewportRef]);

  // 列宽自适应计算
  const visibleColumnCount = table.getVisibleLeafColumns().length;
  const tableBaseWidth = table.getTotalSize();
  const extraWidthPerColumn =
    visibleColumnCount > 0 && viewportWidth > tableBaseWidth
      ? (viewportWidth - tableBaseWidth) / visibleColumnCount
      : 0;
  const tableRenderWidth = Math.max(tableBaseWidth, viewportWidth);

  // 空状态
  if (rows.length === 0) {
    return (
      <div
        className="absolute inset-0 flex flex-col items-center justify-center gap-4"
        data-slot="connections-no-connections"
      >
        <InboxOutlined className="text-surface-variant size-16" />

        <p
          className="text-surface-variant text-sm"
          data-slot="connections-no-connections-message"
        >
          {m.connections_empty_message()}
        </p>
      </div>
    );
  }

  return (
    <div
      className="mx-auto min-h-full"
      data-slot="connections-virtual-container"
      style={{
        height: `${rowVirtualizer.getTotalSize()}px`,
      }}
    >
      <table
        className="divide-outline-variant w-full table-fixed border-separate border-spacing-0"
        data-slot="connections-virtual-table"
        style={{ width: tableRenderWidth }}
      >
        {/* 表头 */}
        <thead className="bg-mixed-background sticky top-0 z-20 h-10">
          {table.getHeaderGroups().map((headerGroup) => (
            <tr key={headerGroup.id}>
              {headerGroup.headers.map((header) => (
                <th
                  key={header.id}
                  colSpan={header.colSpan}
                  className="border-outline-variant relative border-b whitespace-nowrap"
                  style={{ width: header.getSize() + extraWidthPerColumn }}
                >
                  {header.isPlaceholder ? null : (
                    <div
                      className={cn(
                        'truncate px-3 text-left align-middle text-sm font-bold select-none',
                        header.column.getCanSort() &&
                          'hover:text-primary cursor-pointer',
                      )}
                      onClick={header.column.getToggleSortingHandler()}
                    >
                      {flexRender(
                        header.column.columnDef.header,
                        header.getContext(),
                      )}
                      {header.column.getIsSorted() === 'asc' && ' ↑'}
                      {header.column.getIsSorted() === 'desc' && ' ↓'}
                    </div>
                  )}

                  {/* 列宽拖拽手柄 */}
                  {header.column.getCanResize() && (
                    <div
                      onMouseDown={header.getResizeHandler()}
                      onTouchStart={header.getResizeHandler()}
                      className={cn(
                        'absolute top-0 right-0 h-full w-1 cursor-col-resize touch-none select-none',
                        'hover:bg-primary/40 bg-transparent',
                        header.column.getIsResizing() && 'bg-primary/60',
                      )}
                    />
                  )}
                </th>
              ))}
            </tr>
          ))}
        </thead>

        {/* 表体（虚拟行） */}
        <tbody className="select-text" data-slot="connections-virtual-tbody">
          {virtualItems.map((virtualRow, index) => {
            const row = rows[virtualRow.index];

            if (!row) {
              return null;
            }

            const offset = virtualRow.start - index * virtualRow.size;

            return (
              <TableRow
                key={row.id}
                data-index={virtualRow.index}
                ref={(node) => rowVirtualizer.measureElement(node)}
                className={cn(
                  'transition-colors',
                  'hover:bg-primary/5 active:bg-primary/10',
                  row.original.closed && 'opacity-40',
                )}
                style={{
                  height: `${virtualRow.size}px`,
                  transform: `translateY(${offset}px)`,
                }}
                data={row.original}
              >
                {row.getVisibleCells().map(({ column, id, getContext }) => (
                  <td
                    key={id}
                    className="border-outline-variant/30 max-w-0 truncate border-b px-3 text-sm"
                    style={{ width: column.getSize() + extraWidthPerColumn }}
                  >
                    {flexRender(column.columnDef.cell, getContext())}
                  </td>
                ))}
              </TableRow>
            );
          })}
        </tbody>
      </table>
    </div>
  );
}

/**
 * 连接页面主组件
 *
 * 布局：
 * - 顶部：可滚动区域中包含 Viewer（虚拟化表格）
 * - 底部：工具栏（搜索框 + 关闭全部连接按钮）
 * - 使用 useScrollArea 获取 viewportRef 供 Virtualizer 使用
 */
function RouteComponent() {
  const [search, setSearch] = useState('');

  const { deleteConnections } = useClashConnections();

  const handleCloseAllConnections = useLockFn(async () => {
    await deleteConnections.mutateAsync(undefined);
  });

  return (
    <div
      className="divide-outline-variant flex min-h-0 flex-1 flex-col divide-y overflow-hidden"
      data-slot="connections-container"
    >
      {/* 
        可滚动区域（使用 ScrollArea 包裹 Viewer）
        ScrollArea 提供 viewportRef，供 @tanstack/react-virtual 使用
      */}
      <AppContentScrollArea data-slot="connections-scroll-wrapper">
        <Viewer search={search} />
      </AppContentScrollArea>

      {/* 
        底部工具栏：搜索框 + 关闭全部连接
        与 ref 的 toolbar 设计一致
      */}
      <div
        className="bg-mixed-background flex h-16 shrink-0 items-center gap-3 px-4"
        data-slot="connections-toolbar"
      >
        <input
          type="text"
          className={cn(
            'bg-surface-variant dark:bg-surface-variant/30',
            'h-10 min-w-0 flex-1 rounded-full px-4 text-sm outline-none',
          )}
          placeholder={m.connections_search_placeholder()}
          value={search}
          onChange={(e) => setSearch(e.target.value)}
        />

        <Tooltip title={m.connections_close_all_connections()}>
          <IconButton
            size="small"
            color="inherit"
            onClick={handleCloseAllConnections}
          >
            <CloseRounded />
          </IconButton>
        </Tooltip>
      </div>
    </div>
  );
}
