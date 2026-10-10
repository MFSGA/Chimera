import {
  useClashConnectionDetails,
  useTrafficActiveConnectionIds,
  useTrafficClosedConnections,
  type ClashConnection_Serialize,
  type ClosedConnection,
  type TrafficRange,
} from '@chimera/interface';
import {
  useCallback,
  useDeferredValue,
  useMemo,
  useRef,
  useState,
} from 'react';
import { searchableText } from '@/utils/searchable-text';
import {
  toTrafficFilters,
  type SearchFilter,
} from '../../_modules/traffic-filters';
import { activeConnectionDetail } from './table-row';

export type ConnectionRow = ClashConnection_Serialize & {
  // Parsed once per sample: sorting by time compares numbers instead of
  // parsing both dates in every comparison.
  startMs: number;
};

// A connection's other fields are fixed for its life in the core, and its
// relative time follows the table's tick, so only its traffic changes a row.
export const sameTraffic = (a: ConnectionRow, b: ConnectionRow) =>
  a.download === b.download &&
  a.upload === b.upload &&
  a.downloadSpeed === b.downloadSpeed &&
  a.uploadSpeed === b.uploadSpeed;

// The traffic history keeps the process path; the table shows its name.
export const processName = (process: string) =>
  process.split('/').pop() || process;

/** The traffic page's selection, as a jump to the connections page brings it. */
export type ConnectionsSelection = {
  /** Applies to closed connections only, as in the traffic report. */
  range?: TrafficRange;
  filters: SearchFilter[];
};

export const isFiltered = (selection: ConnectionsSelection) =>
  selection.filters.length > 0 || selection.range !== undefined;

// The store keys closed connections by both: a new core reuses ids.
export const closedConnectionId = (row: ClosedConnection) =>
  `${row.closed_at}:${row.id}`;

/**
 * Keeps the rows that go through `proxy` and contain `search`. A row's
 * searchable text never changes, so it is built once and kept by id in
 * `cache` while the row stays.
 */
function filterRows<TRow>(
  rows: TRow[],
  { search, proxy }: { search: string; proxy?: string | null },
  chainsOf: (row: TRow) => string[],
  idOf: (row: TRow) => string,
  cache: { current: Map<string, string> },
) {
  const byProxy = rows.filter((row) =>
    proxy ? chainsOf(row).includes(proxy) : true,
  );

  if (!search) {
    return byProxy;
  }

  const term = search.toLowerCase();
  const previous = cache.current;
  const texts = new Map<string, string>();

  const matched = byProxy.filter((row) => {
    const id = idOf(row);
    const text = previous.get(id) ?? searchableText(row);
    texts.set(id, text);
    return text.includes(term);
  });

  cache.current = texts;

  return matched;
}

const activeChains = (row: ConnectionRow) => row.chains ?? [];

const activeId = (row: ConnectionRow) => row.id;

const closedChains = (row: ClosedConnection) => row.dimensions.chains;

/**
 * The live connections. With `filters`, only those the traffic history
 * matches: none until it says which, so a filtered view never shows all.
 */
export function useActiveConnectionRows({
  search,
  proxy,
  filters,
}: {
  search: string;
  proxy?: string | null;
  filters: SearchFilter[];
}) {
  // As in ref, rendering follows the latest on-demand backend detail frame,
  // independent of the historical Connections recording toggle.
  const { data: latest, status } = useClashConnectionDetails();
  const details = useDeferredValue(latest);
  const filtered = filters.length > 0;
  const ids = useTrafficActiveConnectionIds(toTrafficFilters(filters), {
    enabled: filtered,
  });
  const matchedIds = useMemo(
    () => (!ids.isError && ids.data ? new Set(ids.data) : undefined),
    [ids.isError, ids.data],
  );
  const connections = useMemo<ConnectionRow[]>(() => {
    const live = (details?.connections ?? []) as ClashConnection_Serialize[];
    return live
      .filter((row) => !filtered || matchedIds?.has(row.id))
      .map((row) => ({
        ...row,
        startMs: Date.parse(row.start),
      }));
  }, [details, filtered, matchedIds]);
  const loading =
    (status === 'connecting' && details === null) ||
    (filtered && matchedIds === undefined && !ids.isError);
  const unavailable = status === 'error' || (filtered && ids.isError);

  const searchTexts = useRef(new Map<string, string>());

  const rows = useMemo(
    () =>
      filterRows(
        connections,
        { search, proxy },
        activeChains,
        activeId,
        searchTexts,
      ),
    [connections, search, proxy],
  );

  return { connections, rows, loading, unavailable };
}

/** The closed connections of the selection, a page at a time. */
export function useClosedConnectionRows({
  search,
  proxy,
  selection,
}: {
  search: string;
  proxy?: string | null;
  selection: ConnectionsSelection;
}) {
  const {
    data: history,
    error,
    hasNextPage,
    isFetchingNextPage,
    isFetchNextPageError,
    fetchNextPage,
  } = useTrafficClosedConnections({
    range: selection.range ?? 'all',
    filters: toTrafficFilters(selection.filters),
  });

  // A poll that brings new records shifts the pages, which hands out the
  // others as new objects; the cache keys their text by row id instead.
  const searchTexts = useRef(new Map<string, string>());

  const rows = useMemo(
    () =>
      filterRows(
        (history?.pages ?? []).flatMap((page) => page.connections),
        { search, proxy },
        closedChains,
        closedConnectionId,
        searchTexts,
      ),
    [history, search, proxy],
  );

  // A new function whenever a fetch starts or ends, so an empty table asks
  // again: a page may hold no match yet still lead to older ones.
  const onEndReached = useCallback(() => {
    // A failed page waits for the next poll instead of retrying in a loop.
    if (hasNextPage && !isFetchingNextPage && !isFetchNextPageError) {
      fetchNextPage();
    }
  }, [hasNextPage, isFetchingNextPage, isFetchNextPageError, fetchNextPage]);

  return { rows, error, hasNextPage, onEndReached };
}

/**
 * The details of the live connection `id`, following its samples. The last
 * sample is kept, so the dialog stays on it after the connection closes and
 * leaves the stream. `rowId` is the connection's row id in the opening table.
 */
export function useActiveConnectionDetail(
  connections: ConnectionRow[],
  id: string | null,
  rowId = id,
) {
  // Looked up unfiltered: a search or proxy filter hiding the row does not
  // close the connection.
  const liveRow = useMemo(
    () => (id === null ? undefined : connections.find((row) => row.id === id)),
    [connections, id],
  );

  const [lastRow, setLastRow] = useState<ConnectionRow>();

  if (liveRow && liveRow !== lastRow) {
    setLastRow(liveRow);
  }

  const detailRow = liveRow ?? (lastRow?.id === id ? lastRow : undefined);

  return useMemo(
    () =>
      detailRow && rowId !== null
        ? activeConnectionDetail(detailRow, detailRow !== liveRow, rowId)
        : undefined,
    [detailRow, liveRow, rowId],
  );
}

/**
 * The entry's focus while the page stays on the tab it opened with. Each tab
 * mounts its own table, so a focus kept past a tab switch would be applied
 * again on returning to that tab; the first switch drops it.
 */
export function useTabFocus(scope: string, focus: string | undefined) {
  const [tabFocus, setTabFocus] = useState({ scope, focus });

  if (tabFocus.focus !== undefined && tabFocus.scope !== scope) {
    setTabFocus({ scope, focus: undefined });
  }

  return tabFocus.scope === scope ? tabFocus.focus : undefined;
}
