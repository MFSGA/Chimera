import { useClashRules } from '@chimera/interface';
import HighlightText from '@chimera/ui/highlight-text';
import {
  ScrollArea,
  useScrollAreaViewportElement,
} from '@chimera/ui/scroll-area';
import { SearchField } from '@chimera/ui/search-field';
import { cn } from '@chimera/utils';
import { createFileRoute } from '@tanstack/react-router';
import {
  columnVisibilityFeature,
  createSortedRowModel,
  flexRender,
  rowSortingFeature,
  tableFeatures,
  useTable,
} from '@tanstack/react-table';
import { useVirtualizer } from '@tanstack/react-virtual';
import { useCallback, useDeferredValue, useMemo } from 'react';
import { keepReturn } from '@/components/router/cross-navigation';
import { ReturnButton } from '@/components/router/return-button';
import { useEntryFocus } from '@/components/router/use-cross-navigate';
import * as m from '@/paraglide/messages';
import {
  DataPage,
  DataPageSurface,
  DataPageToolbar,
  useDataPageBottomInset,
} from '../_modules/data-page';
import {
  focusHighlightStyle,
  useFocusHighlight,
} from '../_modules/focus-highlight';
import { ruleLabel } from '../_modules/traffic-filters';
import { useSearchTerm } from '../_modules/use-search-term';
import { Route as IndexRoute } from './route';

export const Route = createFileRoute('/(main)/main/rules/')({
  component: RouteComponent,
});

const features = tableFeatures({
  rowSortingFeature,
  columnVisibilityFeature,
  sortedRowModel: createSortedRowModel(),
});

const Viewer = ({ search, focus }: { search: string; focus?: string }) => {
  const { data } = useClashRules();
  const { proxy } = IndexRoute.useSearch();
  const viewportElement = useScrollAreaViewportElement();
  const bottomInset = useDataPageBottomInset();

  const filteredRules = useMemo(() => {
    const rules = data?.rules ?? [];
    const proxyFilteredRules = proxy
      ? rules.filter((rule) => rule.proxy === proxy)
      : rules;

    if (!search.trim()) return proxyFilteredRules;

    const searchLower = search.toLowerCase();

    return proxyFilteredRules.filter(
      (rule) =>
        rule.type?.toLowerCase().includes(searchLower) ||
        rule.payload?.toLowerCase().includes(searchLower) ||
        rule.proxy?.toLowerCase().includes(searchLower),
    );
  }, [data?.rules, proxy, search]);

  const rowVirtualizer = useVirtualizer({
    count: filteredRules.length,
    paddingEnd: bottomInset,
    scrollPaddingEnd: bottomInset,
    getScrollElement: () => viewportElement,
    estimateSize: () => 56,
    overscan: 10,
    measureElement: (element) => element?.getBoundingClientRect().height,
  });

  const table = useTable({
    features,
    data: filteredRules,
    columns: [
      {
        accessorKey: 'Index',
        header: m.rules_column_index(),
        cell: (info) => info.row.index + 1,
      },
      {
        accessorKey: 'type',
        header: m.rules_column_type(),
        cell: (info) => (
          <HighlightText searchText={search}>
            {info.row.original.type || ''}
          </HighlightText>
        ),
      },
      {
        accessorKey: 'payload',
        header: m.rules_column_payload(),
        cell: (info) => (
          <HighlightText searchText={search}>
            {info.row.original.payload || ''}
          </HighlightText>
        ),
      },
      {
        accessorKey: 'proxy',
        header: m.rules_column_proxy(),
        cell: (info) => (
          <HighlightText searchText={search}>
            {info.row.original.proxy || ''}
          </HighlightText>
        ),
      },
    ],
  });

  const { rows } = table.getRowModel();
  const focusedRule = useFocusHighlight(
    focus,
    rows,
    (row) => ruleLabel(row.original.type, row.original.payload),
    rowVirtualizer,
  );

  if (filteredRules.length === 0) {
    return (
      <div
        className="text-on-surface-variant flex h-full min-h-32 items-center justify-center text-sm"
        data-slot="rules-empty"
      >
        {m.rules_empty_message()}
      </div>
    );
  }

  return (
    <div
      className="mx-auto w-full max-w-7xl px-4"
      data-slot="rules-virtual-container"
      style={{ height: `${rowVirtualizer.getTotalSize()}px` }}
    >
      <table
        className="w-full min-w-0 table-fixed"
        data-slot="rules-virtual-table"
      >
        <colgroup>
          <col className="w-12" />
          <col className="w-36" />
          <col />
          <col className="w-40" />
        </colgroup>
        <tbody className="select-text" data-slot="rules-virtual-tbody">
          {rowVirtualizer.getVirtualItems().map((virtualRow, index) => {
            const row = rows[virtualRow.index];
            const offset = virtualRow.start - index * virtualRow.size;
            const focused =
              ruleLabel(row.original.type, row.original.payload) ===
              focusedRule;

            return (
              <tr
                key={row.id}
                data-index={virtualRow.index}
                data-slot="rules-virtual-tr"
                data-focused={focused}
                className="border-outline-variant/30 hover:bg-on-surface/4 border-b text-sm transition-colors"
                style={{
                  ...(focused && focusHighlightStyle),
                  height: `${virtualRow.size}px`,
                  transform: `translateY(${offset}px)`,
                }}
              >
                {row.getVisibleCells().map((cell, column) => (
                  <td
                    key={cell.id}
                    data-slot="rules-virtual-td"
                    className={cn(
                      'min-w-0 truncate px-2',
                      column === 0 &&
                        'text-on-surface-variant text-right tabular-nums',
                      column === 1 && 'text-on-secondary-container font-medium',
                      column === 3 && 'text-on-surface-variant',
                    )}
                  >
                    {flexRender(cell.column.columnDef.cell, cell.getContext())}
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
  const { q } = IndexRoute.useSearch();
  const navigate = IndexRoute.useNavigate();

  const writeQuery = useCallback(
    (next: string | undefined) =>
      navigate({
        search: (previous) => ({ ...previous, q: next }),
        replace: true,
        state: keepReturn,
      }),
    [navigate],
  );

  const [search, setSearch] = useSearchTerm(q, writeQuery);
  const deferredSearch = useDeferredValue(search);
  const focus = useEntryFocus();

  return (
    <DataPage
      title={m.navbar_label_rules()}
      toolbar={
        <DataPageToolbar data-slot="rules-search">
          <ReturnButton />
          <SearchField
            variant="glass"
            className="min-w-0 flex-1 basis-48"
            data-slot="rules-search-input-field"
            placeholder={m.rules_search_placeholder()}
            aria-label={m.rules_search_placeholder()}
            clearLabel={m.common_clear()}
            value={search}
            onValueChange={setSearch}
          />
        </DataPageToolbar>
      }
    >
      <DataPageSurface className="@container overflow-hidden">
        <ScrollArea className="min-h-0 flex-1" scrollbars="horizontal">
          <div className="flex h-full min-h-0 min-w-[42rem] flex-1 flex-col">
            <div
              className="bg-surface-variant/20 border-outline-variant/40 mx-auto grid h-10 w-full max-w-7xl shrink-0 grid-cols-[3rem_9rem_minmax(0,1fr)_10rem] items-center border-b px-4 text-xs font-medium"
              data-slot="rules-header"
            >
              <span className="text-right">#</span>
              <span className="px-2">{m.rules_column_type()}</span>
              <span className="px-2">{m.rules_column_payload()}</span>
              <span className="px-2">{m.rules_column_proxy()}</span>
            </div>
            <ScrollArea className="min-h-0 flex-1" type="hover">
              <Viewer search={deferredSearch} focus={focus} />
            </ScrollArea>
          </div>
        </ScrollArea>
      </DataPageSurface>
    </DataPage>
  );
}
