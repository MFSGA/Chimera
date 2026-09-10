import {
  useClashConnections,
  type ClashConnectionItem,
} from '@chimera/interface';
import { cn } from '@chimera/utils';
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
import CloseRounded from '~icons/material-symbols/close-rounded';
import dayjs from 'dayjs';
import { useAtomValue } from 'jotai';
import {
  useCallback,
  useEffect,
  useMemo,
  useState,
  type MouseEvent,
  type RefObject,
} from 'react';
import ContentDisplay from '@/components/base/content-display';
import { Button } from '@/components/ui/button';
import HighlightText from '@/components/ui/highlight-text';
import { useLockFn } from '@/hooks/use-lock-fn';
import * as m from '@/paraglide/messages';
import { connectionTableColumnsAtom } from '@/store';
import { containsSearchTerm } from '@/utils';
import parseTraffic from '@/utils/parse-traffic';
import ConnectionDetailDialog from './connection-detail-dialog';

export type TableConnection = ClashConnectionItem & {
  downloadSpeed: number;
  uploadSpeed: number;
};

export const CONNECTION_COLUMNS = [
  ['host', 'Host'],
  ['process', 'Process'],
  ['downloaded', 'Downloaded'],
  ['uploaded', 'Uploaded'],
  ['dl_speed', 'DL Speed'],
  ['ul_speed', 'UL Speed'],
  ['chains', 'Chains'],
  ['rule', 'Rule'],
  ['time', 'Time'],
  ['source', 'Source'],
  ['destination_ip', 'Destination IP'],
  ['destination_asn', 'Destination ASN'],
  ['type', 'Type'],
] as const;

const features = tableFeatures({
  rowSortingFeature,
  columnSizingFeature,
  columnResizingFeature,
  columnVisibilityFeature,
  columnOrderingFeature,
  sortedRowModel: createSortedRowModel(),
});

function CloseConnectionButton({ id }: { id: string }) {
  const { deleteConnections } = useClashConnections();
  const [loading, setLoading] = useState(false);
  const closeConnection = useLockFn(async () => {
    setLoading(true);
    try {
      await deleteConnections.mutateAsync(id);
    } finally {
      setLoading(false);
    }
  });

  return (
    <Button
      icon
      variant="flat"
      loading={loading}
      aria-label={m.connections_close_connection()}
      onClick={(event: MouseEvent<HTMLButtonElement>) => {
        event.preventDefault();
        event.stopPropagation();
        void closeConnection();
      }}
    >
      <CloseRounded className="size-4" />
    </Button>
  );
}

export default function ConnectionsTable({
  searchTerm = '',
  viewportRef,
}: {
  searchTerm?: string;
  viewportRef: RefObject<HTMLElement | null>;
}) {
  const { data: clashConnections, isLoading } = useClashConnections();
  const tableColumns = useAtomValue(connectionTableColumnsAtom);
  const [selected, setSelected] = useState<TableConnection>();
  const [columnSizing, setColumnSizing] = useState<ColumnSizingState>({});

  const data = useMemo<TableConnection[]>(() => {
    const snapshots = clashConnections ?? [];
    const latest = snapshots.at(-1)?.connections ?? [];
    const previous = snapshots.at(-2)?.connections ?? [];
    const previousById = new Map(
      previous.map((connection) => [connection.id, connection]),
    );

    return latest
      .map((connection) => {
        const previousConnection = previousById.get(connection.id);
        return {
          ...connection,
          downloadSpeed: previousConnection
            ? connection.download - previousConnection.download
            : 0,
          uploadSpeed: previousConnection
            ? connection.upload - previousConnection.upload
            : 0,
        };
      })
      .filter((connection) =>
        searchTerm ? containsSearchTerm(connection, searchTerm) : true,
      );
  }, [clashConnections, searchTerm]);

  const columns = useMemo(
    () =>
      [
        {
          id: 'actions',
          header: '',
          size: 60,
          enableSorting: false,
          enableResizing: false,
          cell: ({ row }) => <CloseConnectionButton id={row.original.id} />,
        },
        {
          id: 'host',
          header: 'Host',
          size: 240,
          accessorFn: ({ metadata }) => metadata.host || metadata.destinationIP,
          cell: ({ row }) => (
            <HighlightText
              text={
                row.original.metadata.host ||
                row.original.metadata.destinationIP ||
                ''
              }
              search={searchTerm}
            />
          ),
        },
        {
          id: 'process',
          header: 'Process',
          size: 140,
          accessorFn: ({ metadata }) => metadata.process,
          cell: ({ row }) => (
            <HighlightText
              text={row.original.metadata.process || ''}
              search={searchTerm}
            />
          ),
        },
        {
          id: 'downloaded',
          header: 'Downloaded',
          size: 88,
          accessorFn: ({ download }) => parseTraffic(download).join(' '),
          sortFn: (rowA, rowB) =>
            rowA.original.download - rowB.original.download,
        },
        {
          id: 'uploaded',
          header: 'Uploaded',
          size: 88,
          accessorFn: ({ upload }) => parseTraffic(upload).join(' '),
          sortFn: (rowA, rowB) => rowA.original.upload - rowB.original.upload,
        },
        {
          id: 'dl_speed',
          header: 'DL Speed',
          size: 88,
          accessorFn: ({ downloadSpeed }) =>
            `${parseTraffic(downloadSpeed).join(' ')}/s`,
          sortFn: (rowA, rowB) =>
            rowA.original.downloadSpeed - rowB.original.downloadSpeed,
        },
        {
          id: 'ul_speed',
          header: 'UL Speed',
          size: 88,
          accessorFn: ({ uploadSpeed }) =>
            `${parseTraffic(uploadSpeed).join(' ')}/s`,
          sortFn: (rowA, rowB) =>
            rowA.original.uploadSpeed - rowB.original.uploadSpeed,
        },
        {
          id: 'chains',
          header: 'Chains',
          size: 360,
          accessorFn: ({ chains }) => [...chains].reverse().join(' / '),
          cell: ({ row }) => (
            <HighlightText
              text={[...row.original.chains].reverse().join(' / ')}
              search={searchTerm}
            />
          ),
        },
        {
          id: 'rule',
          header: 'Rule',
          size: 200,
          accessorFn: ({ rule, rulePayload }) =>
            rulePayload ? `${rule} (${rulePayload})` : rule,
          cell: ({ row }) => (
            <HighlightText
              text={
                row.original.rulePayload
                  ? `${row.original.rule} (${row.original.rulePayload})`
                  : row.original.rule || ''
              }
              search={searchTerm}
            />
          ),
        },
        {
          id: 'time',
          header: 'Time',
          size: 120,
          accessorFn: ({ start }) => dayjs(start).fromNow(),
          sortFn: (rowA, rowB) =>
            dayjs(rowA.original.start).diff(rowB.original.start),
          cell: ({ row }) => (
            <span
              title={dayjs(row.original.start).format('YYYY-MM-DD HH:mm:ss')}
            >
              {dayjs(row.original.start).fromNow()}
            </span>
          ),
        },
        {
          id: 'source',
          header: 'Source',
          size: 200,
          accessorFn: ({ metadata: { sourceIP, sourcePort } }) =>
            `${sourceIP}:${sourcePort}`,
        },
        {
          id: 'destination_ip',
          header: 'Destination IP',
          size: 200,
          accessorFn: ({ metadata: { destinationIP, destinationPort } }) =>
            `${destinationIP}:${destinationPort}`,
        },
        {
          id: 'destination_asn',
          header: 'Destination ASN',
          size: 200,
          accessorFn: ({ metadata: { destinationIPASN } }) =>
            String(destinationIPASN ?? ''),
        },
        {
          id: 'type',
          header: 'Type',
          size: 160,
          accessorFn: ({ metadata }) =>
            `${metadata.type} (${metadata.network})`,
        },
      ] satisfies Array<ColumnDef<typeof features, TableConnection>>,
    [searchTerm],
  );

  const columnOrder = useMemo(
    () => [
      'actions',
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

  const handleColumnSizingChange = useCallback(
    (updater: Updater<ColumnSizingState>) => {
      setColumnSizing((previous) =>
        typeof updater === 'function' ? updater(previous) : updater,
      );
    },
    [],
  );

  const table = useTable({
    features,
    data,
    columns,
    state: { columnOrder, columnVisibility, columnSizing },
    onColumnSizingChange: handleColumnSizingChange,
    enableColumnResizing: true,
    columnResizeMode: 'onChange',
  });
  const { rows } = table.getRowModel();

  const rowVirtualizer = useVirtualizer({
    count: rows.length,
    getScrollElement: () => viewportRef.current,
    estimateSize: () => 40,
    overscan: 10,
    measureElement: (element) => element?.getBoundingClientRect().height,
  });
  const virtualItems = rowVirtualizer.getVirtualItems();

  const [viewportWidth, setViewportWidth] = useState(0);
  useEffect(() => {
    const viewport = viewportRef.current;
    if (!viewport) return;

    const updateWidth = () => setViewportWidth(viewport.clientWidth);
    updateWidth();

    const observer = new ResizeObserver(updateWidth);
    observer.observe(viewport);
    return () => observer.disconnect();
  }, [viewportRef]);

  const visibleColumnCount = table.getVisibleLeafColumns().length;
  const tableBaseWidth = table.getTotalSize();
  const extraWidthPerColumn =
    visibleColumnCount > 0 && viewportWidth > tableBaseWidth
      ? (viewportWidth - tableBaseWidth) / visibleColumnCount
      : 0;
  const tableRenderWidth = Math.max(tableBaseWidth, viewportWidth);

  if (isLoading && !clashConnections?.length) {
    return null;
  }

  if (rows.length === 0) {
    return (
      <ContentDisplay
        className="absolute"
        message={m.connections_empty_message()}
      />
    );
  }

  return (
    <>
      <ConnectionDetailDialog
        item={selected}
        open={!!selected}
        onClose={() => setSelected(undefined)}
      />

      <div
        className="mx-auto min-h-full"
        data-slot="legacy-connections-virtual-container"
        style={{ height: `${rowVirtualizer.getTotalSize()}px` }}
      >
        <table
          className="divide-outline-variant w-full table-fixed border-separate border-spacing-0"
          data-slot="legacy-connections-virtual-table"
          style={{ width: tableRenderWidth }}
        >
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
                      <button
                        type="button"
                        className={cn(
                          'w-full truncate px-3 text-left align-middle text-sm font-bold select-none',
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
                      </button>
                    )}

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

          <tbody className="select-text">
            {virtualItems.map((virtualRow, index) => {
              const row = rows[virtualRow.index];
              if (!row) return null;
              const offset = virtualRow.start - index * virtualRow.size;

              return (
                <tr
                  key={row.id}
                  data-index={virtualRow.index}
                  ref={(node) => rowVirtualizer.measureElement(node)}
                  className="hover:bg-primary/5 active:bg-primary/10 cursor-pointer transition-colors"
                  style={{
                    height: `${virtualRow.size}px`,
                    transform: `translateY(${offset}px)`,
                  }}
                  onClick={() => setSelected(row.original)}
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
                </tr>
              );
            })}
          </tbody>
        </table>
      </div>
    </>
  );
}
