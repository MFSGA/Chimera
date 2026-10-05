/**
 * Displays the current Clash connections alongside persisted traffic history.
 * The traffic API follows ref's traffic actor and query model; this existing
 * Chimera table remains a temporary presentation adapter while the ref's
 * ActiveViewer / AllViewer / ClosedViewer decomposition is migrated.
 */

import {
  useClashConnections,
  useTrafficActiveConnectionIds,
  useTrafficClosedConnections,
  type ClashConnectionItem,
  type ClashConnectionMetadata,
  type ClosedConnection,
  type TrafficFilter,
  type TrafficRange,
  type TrafficScope,
} from '@chimera/interface';
import { cn } from '@chimera/ui';
import { createFileRoute } from '@tanstack/react-router';
import {
  columnOrderingFeature,
  columnResizingFeature,
  columnSizingFeature,
  columnVisibilityFeature,
  createSortedRowModel,
  flexRender,
  rowSortingFeature,
  tableFeatures,
  useTable,
  type ColumnDef,
  type ColumnSizingState,
  type Updater,
} from '@tanstack/react-table';
import { useVirtualizer } from '@tanstack/react-virtual';
import BoxOutlineRounded from '~icons/material-symbols/box-outline-rounded';
import CloseRounded from '~icons/material-symbols/close-rounded';
import dayjs from 'dayjs';
import relativeTime from 'dayjs/plugin/relativeTime';
import { useAtomValue } from 'jotai';
import {
  useCallback,
  useDeferredValue,
  useEffect,
  useMemo,
  useState,
} from 'react';
import ConnectionColumnFilterDialog from '@/components/connections/connections-column-filter';
import { CONNECTION_COLUMNS } from '@/components/connections/connections-table';
import {
  RegisterContextMenu,
  RegisterContextMenuContent,
  RegisterContextMenuTrigger,
} from '@/components/providers/context-menu-provider';
import { ContextMenuItem } from '@/components/ui/context-menu';
import HighlightText from '@/components/ui/highlight-text';
import { ScrollArea, useScrollArea } from '@/components/ui/scroll-area';
import { useLocalStorage } from '@/hooks/use-local-storage';
import { useLockFn } from '@/hooks/use-lock-fn';
import * as m from '@/paraglide/messages';
import { connectionTableColumnsAtom } from '@/store';
import { containsSearchTerm } from '@/utils';
import parseTraffic from '@/utils/parse-traffic';
import {
  toTrafficFilters,
  type SearchFilter,
} from '../_modules/traffic-filters';
import { useSearchTerm } from '../_modules/use-search-term';
import ConnectionsToolbar from './_modules/connections-toolbar';
import ConnectionsStatusTabs from './_modules/status-tabs';
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
  closedAt?: number;
  downloadSpeed: number;
  uploadSpeed: number;
};

function closedConnectionRow(connection: ClosedConnection): ConnectionRow {
  const dimensions = connection.dimensions;
  const metadata: ClashConnectionMetadata = {
    network: dimensions.protocol,
    type: '',
    host: dimensions.target,
    sourceIP: dimensions.source,
    sourcePort: '',
    destinationPort: '',
    process: dimensions.process,
    processPath: dimensions.process,
    inboundName: dimensions.inbound,
  };

  return {
    id: `${connection.closed_at}:${connection.id}`,
    metadata,
    upload: connection.bytes.upload,
    download: connection.bytes.download,
    start: new Date(connection.started_at).toISOString(),
    chains: dimensions.chains,
    rule: dimensions.rule.kind,
    rulePayload: dimensions.rule.payload,
    closed: true,
    closedAt: connection.closed_at,
    downloadSpeed: 0,
    uploadSpeed: 0,
  };
}

const features = tableFeatures({
  rowSortingFeature,
  columnOrderingFeature,
  columnSizingFeature,
  columnResizingFeature,
  columnVisibilityFeature,
  sortedRowModel: createSortedRowModel(),
});

/**
 * 列宽配置 localStorage 键名
 * 与 ref 保持一致：'connections-column-sizing-v2'
 */
const COLUMN_SIZING_STORAGE_KEY = 'connections-column-sizing-v2';

export const Route = createFileRoute('/(main)/main/connections/')({
  component: RouteComponent,
});

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
  const {
    proxy,
    scope = 'active',
    range,
    filters: searchFilters = [],
  } = ConnectionsRoute.useSearch();
  const filters = useMemo(
    () => toTrafficFilters(searchFilters),
    [searchFilters],
  );
  const filtered = filters.length > 0;

  // 列宽状态（持久化到 localStorage）
  const [columnSizing, setColumnSizing] = useLocalStorage<ColumnSizingState>(
    COLUMN_SIZING_STORAGE_KEY,
    {},
  );

  // WebSocket 连接数据
  const { data: clashConnections } = useClashConnections();
  const activeIds = useTrafficActiveConnectionIds(filters, {
    enabled: filtered && scope !== 'closed',
  });
  const closedQuery = useTrafficClosedConnections({
    range: range ?? 'all',
    filters,
    enabled: scope !== 'active',
  });
  const closedConnections = useMemo(
    () => closedQuery.data?.pages.flatMap((page) => page.connections) ?? [],
    [closedQuery.data],
  );

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

    const matchedActiveIds = filtered ? new Set(activeIds.data ?? []) : null;
    const live = latestConnections
      .filter((conn) => !matchedActiveIds || matchedActiveIds.has(conn.id))
      .filter((conn) => (proxy ? conn.chains?.includes(proxy) : true))
      .map((conn) => {
        const prev = prevMap.get(conn.id);
        return {
          ...conn,
          closed: false,
          downloadSpeed: prev ? conn.download - prev.download : 0,
          uploadSpeed: prev ? conn.upload - prev.upload : 0,
        };
      });

    const liveIds = new Set(
      latestConnections.map((connection) => connection.id),
    );
    const closed = closedConnections
      .filter((connection) => !liveIds.has(connection.id))
      .map(closedConnectionRow)
      .filter((connection) =>
        proxy ? connection.chains?.includes(proxy) : true,
      );

    const selected =
      scope === 'active'
        ? live
        : scope === 'closed'
          ? closed
          : [...live, ...closed];

    return selected.filter((connection) =>
      search ? containsSearchTerm(connection, search) : true,
    );
  }, [
    activeIds.data,
    clashConnections,
    closedConnections,
    filtered,
    proxy,
    scope,
    search,
  ]);

  /**
   * 表格列定义
   * 与 ref 一致：Host / Chains / Downloaded / Uploaded / DL Speed / UL Speed / Process / Rule / Time / Source / Destination IP / Type
   */
  const columns = useMemo(
    () =>
      [
        {
          id: 'host',
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
          id: 'chains',
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
          id: 'downloaded',
          header: 'Downloaded',
          accessorFn: ({ download }) => parseTraffic(download).join(' '),
          sortFn: (rowA, rowB) =>
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
          id: 'uploaded',
          header: 'Uploaded',
          accessorFn: ({ upload }) => parseTraffic(upload).join(' '),
          sortFn: (rowA, rowB) => rowA.original.upload - rowB.original.upload,
          size: 120,
          cell: (info) => (
            <span>{parseTraffic(info.row.original.upload).join(' ')}</span>
          ),
        },
        {
          id: 'dl_speed',
          header: 'DL Speed',
          accessorFn: ({ downloadSpeed }) =>
            parseTraffic(downloadSpeed).join(' ') + '/s',
          sortFn: (rowA, rowB) =>
            rowA.original.downloadSpeed - rowB.original.downloadSpeed,
          size: 120,
          cell: (info) => (
            <span>
              {parseTraffic(info.row.original.downloadSpeed).join(' ')}/s
            </span>
          ),
        },
        {
          id: 'ul_speed',
          header: 'UL Speed',
          accessorFn: ({ uploadSpeed }) =>
            parseTraffic(uploadSpeed).join(' ') + '/s',
          sortFn: (rowA, rowB) =>
            rowA.original.uploadSpeed - rowB.original.uploadSpeed,
          size: 120,
          cell: (info) => (
            <span>
              {parseTraffic(info.row.original.uploadSpeed).join(' ')}/s
            </span>
          ),
        },
        {
          id: 'process',
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
          id: 'rule',
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
          id: 'time',
          header: 'Time',
          accessorFn: ({ start }) => dayjs(start).fromNow(),
          sortFn: (rowA, rowB) =>
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
          id: 'source',
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
          id: 'destination_ip',
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
          id: 'destination_asn',
          header: 'Destination ASN',
          accessorFn: ({ metadata: { destinationIPASN } }) =>
            String(destinationIPASN ?? ''),
          size: 160,
          cell: (info) => (
            <HighlightText
              text={String(info.row.original.metadata.destinationIPASN ?? '')}
              search={search}
            />
          ),
        },
        {
          id: 'type',
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
      ] satisfies Array<ColumnDef<typeof features, ConnectionRow>>,
    [search],
  );

  const tableColumns = useAtomValue(connectionTableColumnsAtom);
  const columnOrder = useMemo(
    () => [
      ...tableColumns.map(([id]) => id),
      ...CONNECTION_COLUMNS.map(([id]) => id).filter(
        (id) => !tableColumns.some(([storedId]) => storedId === id),
      ),
    ],
    [tableColumns],
  );
  const columnVisibility = useMemo(
    () =>
      Object.fromEntries(tableColumns.map(([id, visible]) => [id, visible])),
    [tableColumns],
  );

  // 初始化 @tanstack/react-table
  const table = useTable({
    features,
    data,
    columns,
    state: {
      columnSizing,
      columnOrder,
      columnVisibility,
    },
    onColumnSizingChange: handleColumnSizingChange,
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
  const fetchNextPage = closedQuery.fetchNextPage;
  const hasNextPage = closedQuery.hasNextPage;
  const isFetchingNextPage = closedQuery.isFetchingNextPage;
  const lastVirtualIndex = virtualItems.at(-1)?.index ?? -1;

  useEffect(() => {
    if (
      scope !== 'active' &&
      hasNextPage &&
      !isFetchingNextPage &&
      lastVirtualIndex >= rows.length - 1
    ) {
      void fetchNextPage();
    }
  }, [
    fetchNextPage,
    hasNextPage,
    isFetchingNextPage,
    lastVirtualIndex,
    rows.length,
    scope,
  ]);

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
    const loadingHistory = scope !== 'active' && closedQuery.isPending;
    const filterUnavailable =
      (scope !== 'closed' && filtered && activeIds.isError) ||
      (scope !== 'active' && closedQuery.isError);
    return (
      <div
        className="absolute inset-0 flex flex-col items-center justify-center gap-4"
        data-slot="connections-no-connections"
      >
        <BoxOutlineRounded className="text-surface-variant size-16" />

        <p
          className="text-surface-variant text-sm"
          data-slot="connections-no-connections-message"
        >
          {loadingHistory
            ? m.connections_history_loading()
            : filterUnavailable
              ? m.connections_filter_unavailable()
              : m.connections_empty_message()}
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
                {row.getVisibleCells().map((cell) => (
                  <td
                    key={cell.id}
                    className="border-outline-variant/30 max-w-0 truncate border-b px-3 text-sm"
                    style={{
                      width: cell.column.getSize() + extraWidthPerColumn,
                    }}
                  >
                    {flexRender(cell.column.columnDef.cell, cell.getContext())}
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
  const {
    q,
    scope = 'active',
    range,
    filters = [],
  } = ConnectionsRoute.useSearch();
  const navigate = ConnectionsRoute.useNavigate();
  const [search, setSearch] = useSearchTerm(
    q,
    useCallback(
      (next) =>
        navigate({
          search: (previous) => ({ ...previous, q: next }),
          replace: true,
        }),
      [navigate],
    ),
  );
  const deferredSearch = useDeferredValue(search);
  const showTable = useDeferredValue(true, false);

  const updateSelection = (patch: {
    range?: TrafficRange | null;
    filters?: SearchFilter[];
  }) =>
    navigate({
      search: (previous) => ({
        ...previous,
        range:
          patch.range === null ? undefined : (patch.range ?? previous.range),
        filters: patch.filters
          ? patch.filters.length > 0
            ? patch.filters
            : undefined
          : previous.filters,
      }),
      replace: true,
    });

  const [settingsOpen, setSettingsOpen] = useState(false);

  const { deleteConnections } = useClashConnections();

  const handleCloseAllConnections = useLockFn(async () => {
    await deleteConnections.mutateAsync(undefined);
  });

  const scrollArea = (
    <ScrollArea
      key={scope}
      className="min-h-0 flex-1"
      scrollbars="both"
      type="hover"
      data-slot="connections-scroll-wrapper"
    >
      {showTable && <Viewer search={deferredSearch} />}
    </ScrollArea>
  );

  return (
    <>
      <div
        className="divide-outline-variant flex min-h-0 flex-1 flex-col divide-y overflow-hidden"
        data-slot="connections-container"
      >
        {scope === 'closed' ? (
          scrollArea
        ) : (
          <RegisterContextMenu>
            <RegisterContextMenuTrigger asChild>
              {scrollArea}
            </RegisterContextMenuTrigger>
            <RegisterContextMenuContent>
              <ContextMenuItem onSelect={() => handleCloseAllConnections()}>
                <CloseRounded className="size-4" />
                <span>{m.connections_close_all_connections()}</span>
              </ContextMenuItem>
            </RegisterContextMenuContent>
          </RegisterContextMenu>
        )}

        <ConnectionsToolbar
          tabs={
            <ConnectionsStatusTabs
              value={scope}
              onValueChange={(next) =>
                navigate({
                  search: (previous) => ({ ...previous, scope: next }),
                })
              }
              selection={{ range, filters }}
            />
          }
          selection={{ range, filters }}
          onSelectionChange={(next) =>
            updateSelection({
              range: next.range ?? null,
              filters: next.filters,
            })
          }
          search={search}
          onSearchChange={setSearch}
          onOpenSettings={() => setSettingsOpen(true)}
          onCloseAll={
            scope === 'closed' ? undefined : handleCloseAllConnections
          }
        />
      </div>
      <ConnectionColumnFilterDialog
        open={settingsOpen}
        onClose={() => setSettingsOpen(false)}
      />
    </>
  );
}
