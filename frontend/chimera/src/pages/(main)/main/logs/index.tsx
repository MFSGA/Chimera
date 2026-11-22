import { useClashLogs, type ClashLog } from '@chimera/interface';
import { Button } from '@chimera/ui/button';
import { ContextMenuItem } from '@chimera/ui/context-menu';
import { EmptyState } from '@chimera/ui/empty-state';
import {
  ScrollArea,
  useScrollArea,
  useScrollAreaViewportElement,
} from '@chimera/ui/scroll-area';
import { createFileRoute } from '@tanstack/react-router';
import { useVirtualizer } from '@tanstack/react-virtual';
import DeleteForeverOutlineRounded from '~icons/material-symbols/delete-forever-outline-rounded';
import { useReducedMotion } from 'motion/react';
import { useDeferredValue, useEffect, useMemo, useRef, useState } from 'react';
import {
  RegisterContextMenu,
  RegisterContextMenuContent,
  RegisterContextMenuTrigger,
} from '@/components/providers/context-menu-provider';
import { useLockFn } from '@/hooks/use-lock-fn';
import * as m from '@/paraglide/messages';
import { useDataPageBottomInset } from '../_modules/data-page';
import { matchesLogLevel } from './_modules/consts';
import {
  LogClearButton,
  LogFollowButton,
  logPanelVariants,
  LogRecord,
  LogSearch,
  LogVirtualItem,
  useEnteringRows,
  useSlowLoading,
} from './_modules/log-viewer';
import { LogsLayout } from './_modules/logs-layout';
import { Route as IndexRoute } from './route';

export const Route = createFileRoute('/(main)/main/logs/')({
  component: RouteComponent,
});

type CoreLogs = ReturnType<typeof useClashLogs>;

const neverAdjust = () => false;

// CoreLogCursor is not exposed by the legacy WS API. Preserve object identity
// across append/capped-history updates until the cursor RPC is migrated.
const logIds = new WeakMap<ClashLog, number>();
let nextLogId = 0;
const logRowKey = (log: ClashLog) => {
  let id = logIds.get(log);
  if (id === undefined) {
    id = ++nextLogId;
    logIds.set(log, id);
  }
  return String(id);
};

const Viewer = ({
  logs,
  search,
  following,
  setFollowing,
}: {
  logs: CoreLogs;
  search: string;
  following: boolean;
  setFollowing: (following: boolean) => void;
}) => {
  const { level } = IndexRoute.useSearch();

  const filteredLogs = useMemo(() => {
    if (!logs.data) {
      return [];
    }

    const keyword = search.trim().toLowerCase();
    return logs.data.filter((log) => {
      if (!matchesLogLevel(log.type, level)) return false;
      if (!keyword) return true;
      return [log.time, log.type, log.payload].some((part) =>
        part?.toLowerCase().includes(keyword),
      );
    });
  }, [logs.data, level, search]);

  // Defer high-frequency log rows so text input and navigation remain responsive.
  // Keep the rows and their virtual indexes on the same deferred snapshot.
  const rows = useDeferredValue(filteredLogs);
  const deferredSearch = useDeferredValue(search);

  const { scrollDirection, isBottom } = useScrollArea();
  const reducedMotion = useReducedMotion();
  const bottomInset = useDataPageBottomInset();
  const followBehavior = useRef<ScrollBehavior>('auto');
  const isEntering = useEnteringRows(rows, logRowKey);
  const viewportElement = useScrollAreaViewportElement();

  const rowVirtualizer = useVirtualizer({
    count: rows.length,
    getItemKey: (index) => logRowKey(rows[index]),
    getScrollElement: () => viewportElement,
    estimateSize: () => 88,
    overscan: 5,
    paddingEnd: bottomInset,
    scrollPaddingEnd: bottomInset,
    useFlushSync: false,
    useAnimationFrameWithResizeObserver: true,
  });

  // Variable-height records must not pull a live-following viewport upward.
  rowVirtualizer.shouldAdjustScrollPositionOnItemSizeChange = following
    ? neverAdjust
    : undefined;

  const totalSize = rowVirtualizer.getTotalSize();
  const virtualItems = rowVirtualizer.getVirtualItems();

  // Re-measure variable-height rows when the virtualizer's viewport attaches.
  useEffect(() => {
    rowVirtualizer.measure();
  }, [rowVirtualizer]);

  // An intentional upward scroll pauses following, as in the reference viewer.
  useEffect(() => {
    if (scrollDirection === 'up' && !isBottom) setFollowing(false);
  }, [scrollDirection, isBottom, setFollowing]);

  useEffect(() => {
    if (!following || !rows.length) return;
    const frame = requestAnimationFrame(() => {
      rowVirtualizer.scrollToIndex(rows.length - 1, {
        align: 'end',
        behavior: reducedMotion ? 'auto' : followBehavior.current,
      });
      followBehavior.current = 'smooth';
    });
    return () => cancelAnimationFrame(frame);
  }, [following, rows.length, rowVirtualizer, reducedMotion, totalSize]);

  if (filteredLogs.length === 0 && !logs.isLoading && !logs.error) {
    return (
      <EmptyState
        className="absolute inset-0"
        data-slot="logs-no-logs"
        message={
          <span data-slot="logs-no-logs-message">{m.logs_empty_message()}</span>
        }
      />
    );
  }

  return (
    <div
      className="relative"
      data-slot="logs-virtual-list"
      style={{ height: `${totalSize}px` }}
    >
      {virtualItems.map((virtualItem) => {
        const log = rows[virtualItem.index];

        if (!log) {
          return null;
        }

        return (
          <LogVirtualItem
            key={virtualItem.key}
            ref={rowVirtualizer.measureElement}
            data-index={virtualItem.index}
            entering={isEntering(log)}
            style={{ transform: `translateY(${virtualItem.start}px)` }}
          >
            <LogRecord
              time={log.time || ''}
              level={log.type}
              message={log.payload}
              raw={log}
              search={deferredSearch}
              onInspect={() => setFollowing(false)}
            />
          </LogVirtualItem>
        );
      })}
    </div>
  );
};

function RouteComponent() {
  const [search, setSearch] = useState('');
  const [following, setFollowing] = useState(true);
  const logs = useClashLogs();
  const { level } = IndexRoute.useSearch();
  const slowLoading = useSlowLoading(logs.isLoading);

  useEffect(() => setFollowing(true), [level]);

  const handleClearLogs = useLockFn(async () => {
    await logs.clean.mutateAsync();
  });

  return (
    <LogsLayout
      search={
        <LogSearch
          value={search}
          onChange={(value) => {
            setSearch(value);
            setFollowing(true);
          }}
          placeholder={m.logs_search_placeholder()}
        />
      }
      actions={
        <LogClearButton
          disabled={logs.isLoading || !logs.data.length}
          onClick={() => void handleClearLogs()}
          label={m.logs_action_clear_log()}
        />
      }
    >
      <div className={logPanelVariants()} data-slot="core-logs">
        {!following && !!logs.data.length && (
          <LogFollowButton onClick={() => setFollowing(true)} />
        )}
        {(slowLoading || Boolean(logs.error)) && (
          <div
            className="text-on-surface-variant flex flex-wrap items-center gap-2 px-4 py-2 text-xs"
            role="status"
            data-slot="logs-status"
          >
            {slowLoading && <span>{m.logs_loading()}</span>}
            {Boolean(logs.error) && (
              <>
                <span className="text-error" role="alert">
                  {m.logs_source_unavailable()}
                </span>
                <Button
                  data-slot="logs-retry"
                  variant="flat"
                  onClick={() => void logs.retry()}
                >
                  {m.logs_retry()}
                </Button>
              </>
            )}
          </div>
        )}
        <RegisterContextMenu>
          <RegisterContextMenuTrigger asChild>
            <ScrollArea className="min-h-0 flex-1" data-slot="logs-scroll-area">
              <Viewer
                logs={logs}
                search={search}
                following={following}
                setFollowing={setFollowing}
              />
            </ScrollArea>
          </RegisterContextMenuTrigger>
          <RegisterContextMenuContent>
            <ContextMenuItem
              disabled={logs.isLoading || !logs.data.length}
              onClick={handleClearLogs}
            >
              <DeleteForeverOutlineRounded aria-hidden className="size-4" />
              <span>{m.logs_action_clear_log()}</span>
            </ContextMenuItem>
          </RegisterContextMenuContent>
        </RegisterContextMenu>
      </div>
    </LogsLayout>
  );
}
