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
import { useMemo, useState } from 'react';
import { ScrollArea, useScrollArea } from '@/components/ui/scroll-area';
import { Route as IndexRoute } from './route';

export const Route = createFileRoute('/(main)/main/rules/')({
  component: RouteComponent,
});

function HighlightText({
  searchText,
  children,
}: {
  searchText: string;
  children: string;
}) {
  if (!searchText.trim()) {
    return <span>{children}</span>;
  }

  const parts: { text: string; isHighlight: boolean }[] = [];
  const searchLower = searchText.toLowerCase();
  const textLower = children.toLowerCase();
  let lastIndex = 0;
  let index = textLower.indexOf(searchLower, lastIndex);

  while (index !== -1) {
    if (index > lastIndex) {
      parts.push({
        text: children.slice(lastIndex, index),
        isHighlight: false,
      });
    }

    parts.push({
      text: children.slice(index, index + searchText.length),
      isHighlight: true,
    });
    lastIndex = index + searchText.length;
    index = textLower.indexOf(searchLower, lastIndex);
  }

  if (lastIndex < children.length) {
    parts.push({ text: children.slice(lastIndex), isHighlight: false });
  }

  return (
    <span>
      {parts.map((part, partIndex) =>
        part.isHighlight ? (
          <mark
            key={partIndex}
            className="rounded bg-yellow-400 px-0.5 text-black dark:bg-yellow-500"
          >
            {part.text}
          </mark>
        ) : (
          <span key={partIndex}>{part.text}</span>
        ),
      )}
    </span>
  );
}

const Viewer = ({ search }: { search: string }) => {
  const { data } = useClashRules();
  const { proxy } = IndexRoute.useSearch();
  const { viewportRef } = useScrollArea();

  const filteredRules = useMemo(() => {
    const rules = data?.rules ?? [];

    const proxyFilteredRules = proxy
      ? rules.filter((rule) => rule.proxy === proxy)
      : rules;

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

  const rowVirtualizer = useVirtualizer({
    count: filteredRules.length,
    getScrollElement: () => viewportRef.current,
    estimateSize: () => 48,
    overscan: 10,
    measureElement: (element) => element?.getBoundingClientRect().height,
  });

  const virtualItems = rowVirtualizer.getVirtualItems();

  const table = useReactTable({
    data: filteredRules,
    columns: [
      {
        accessorKey: 'Index',
        header: 'Index',
        cell: (info) => info.row.index + 1,
      },
      {
        accessorKey: 'type',
        header: 'Type',
        cell: (info) => (
          <HighlightText searchText={search}>
            {info.row.original.type || ''}
          </HighlightText>
        ),
      },
      {
        accessorKey: 'payload',
        header: 'Payload',
        cell: (info) => (
          <HighlightText searchText={search}>
            {info.row.original.payload || ''}
          </HighlightText>
        ),
      },
      {
        accessorKey: 'proxy',
        header: 'Proxy',
        cell: (info) => (
          <HighlightText searchText={search}>
            {info.row.original.proxy || ''}
          </HighlightText>
        ),
      },
    ],
    getCoreRowModel: getCoreRowModel(),
    getSortedRowModel: getSortedRowModel(),
    debugTable: true,
  });

  const { rows } = table.getRowModel();

  return (
    <div
      className="mx-auto max-w-7xl px-8"
      data-slot="rules-virtual-container"
      style={{ height: `${rowVirtualizer.getTotalSize()}px` }}
    >
      <table
        className="w-full min-w-208 table-fixed"
        data-slot="rules-virtual-table"
      >
        <colgroup>
          <col className="w-20" />
          <col className="w-40" />
          <col />
          <col className="w-40" />
        </colgroup>

        <tbody className="select-text" data-slot="rules-virtual-tbody">
          {virtualItems.map((virtualRow, index) => {
            const row = rows[virtualRow.index];
            const offset = virtualRow.start - index * virtualRow.size;

            return (
              <tr
                key={row.id}
                data-index={virtualRow.index}
                data-slot="rules-virtual-tr"
                style={{
                  height: `${virtualRow.size}px`,
                  transform: `translateY(${offset}px)`,
                }}
              >
                {row.getVisibleCells().map(({ column, id, getContext }) => (
                  <td key={id} data-slot="rules-virtual-td">
                    {flexRender(column.columnDef.cell, getContext())}
                  </td>
                ))}
              </tr>
            );
          })}
        </tbody>
      </table>
    </div>
  );
};

function RouteComponent() {
  const [search, setSearch] = useState('');

  return (
    <div className="divide-outline-variant flex min-h-0 flex-1 flex-col divide-y overflow-hidden">
      <ScrollArea className="min-h-0 flex-1" scrollbars="both" type="hover">
        <Viewer search={search} />
      </ScrollArea>

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
          placeholder="Search rules (type, payload, or proxy)..."
          value={search}
          onChange={(e) => setSearch(e.target.value)}
        />
      </div>
    </div>
  );
}
