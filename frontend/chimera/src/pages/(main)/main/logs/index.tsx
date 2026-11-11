import { useClashLogs, type ClashLog } from '@chimera/interface';
import { ContextMenuItem } from '@chimera/ui/context-menu';
import { GlassControlGroup } from '@chimera/ui/glass-control-group';
import {
  ScrollArea,
  useScrollArea,
  useScrollAreaViewportElement,
} from '@chimera/ui/scroll-area';
import { SearchField } from '@chimera/ui/search-field';
import { createFileRoute } from '@tanstack/react-router';
import { useVirtualizer } from '@tanstack/react-virtual';
import BoxOutlineRounded from '~icons/material-symbols/box-outline-rounded';
import DeleteForeverOutlineRounded from '~icons/material-symbols/delete-forever-outline-rounded';
import { useReducedMotion } from 'motion/react';
import { useEffect, useMemo, useRef, useState } from 'react';
import {
  RegisterContextMenu,
  RegisterContextMenuContent,
  RegisterContextMenuTrigger,
} from '@/components/providers/context-menu-provider';
import { useLockFn } from '@/hooks/use-lock-fn';
import * as m from '@/paraglide/messages';
import {
  DataPage,
  DataPageSurface,
  DataPageToolbar,
  useDataPageBottomInset,
} from '../_modules/data-page';
import { matchesLogLevel } from './_modules/consts';
import {
  LogClearButton,
  LogFollowButton,
  logPanelVariants,
  LogRecord,
  LogVirtualItem,
  useEnteringRows,
} from './_modules/log-viewer';
import { Route as IndexRoute, LogLevelsLayout } from './route';

export const Route = createFileRoute('/(main)/main/logs/')({
  component: RouteComponent,
});

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
  search,
  following,
  setFollowing,
}: {
  search: string;
  following: boolean;
  setFollowing: (following: boolean) => void;
}) => {
  const { level } = IndexRoute.useSearch();
  const { data: logs } = useClashLogs();

  const filteredLogs = useMemo(() => {
    if (!logs) {
      return [];
    }

    const keyword = search.trim().toLowerCase();
    return logs.filter((log) => {
      if (!matchesLogLevel(log.type, level)) return false;
      if (!keyword) return true;
      return [log.time, log.type, log.payload].some((part) =>
        part?.toLowerCase().includes(keyword),
      );
    });
  }, [logs, level, search]);

  const { scrollDirection, isBottom } = useScrollArea();
  const reducedMotion = useReducedMotion();
  const bottomInset = useDataPageBottomInset();
  const followBehavior = useRef<ScrollBehavior>('auto');
  const isEntering = useEnteringRows(filteredLogs, logRowKey);
  const viewportElement = useScrollAreaViewportElement();

  const rowVirtualizer = useVirtualizer({
    count: filteredLogs.length,
    getItemKey: (index) => logRowKey(filteredLogs[index]),
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

  // An intentional upward scroll pauses following, as in the reference viewer.
  useEffect(() => {
    if (scrollDirection === 'up' && !isBottom) setFollowing(false);
  }, [scrollDirection, isBottom, setFollowing]);

  useEffect(() => {
    if (!following || !filteredLogs.length) return;
    const frame = requestAnimationFrame(() => {
      rowVirtualizer.scrollToIndex(filteredLogs.length - 1, {
        align: 'end',
        behavior: reducedMotion ? 'auto' : followBehavior.current,
      });
      followBehavior.current = 'smooth';
    });
    return () => cancelAnimationFrame(frame);
  }, [
    following,
    filteredLogs.length,
    rowVirtualizer,
    reducedMotion,
    totalSize,
  ]);

  if (filteredLogs.length === 0) {
    return (
      <div
        className="absolute inset-0 flex flex-col items-center justify-center gap-4"
        data-slot="logs-no-logs"
      >
        <BoxOutlineRounded className="text-surface-variant size-16" />

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
      className="relative"
      data-slot="logs-virtual-list"
      style={{ height: `${totalSize}px` }}
    >
      {virtualItems.map((virtualItem) => {
        const log = filteredLogs[virtualItem.index];

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
              search={search}
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
  const { data: logs, clean } = useClashLogs();

  const handleClearLogs = useLockFn(async () => {
    await clean.mutateAsync();
  });

  return (
    <LogLevelsLayout>
      <DataPage
        title={m.navbar_label_logs()}
        toolbar={
          <DataPageToolbar data-slot="logs-toolbar">
            <div className="min-w-32 flex-1" data-slot="logs-search">
              <SearchField
                variant="glass"
                className="w-full"
                data-slot="logs-search-input-field"
                value={search}
                onValueChange={(value) => {
                  setSearch(value);
                  setFollowing(true);
                }}
                placeholder={m.logs_search_placeholder()}
                aria-label={m.logs_search_placeholder()}
                clearLabel={m.common_clear()}
              />
            </div>
            <GlassControlGroup data-slot="logs-actions">
              <LogClearButton
                disabled={!logs?.length}
                onClick={() => void handleClearLogs()}
                label={m.logs_action_clear_log()}
              />
            </GlassControlGroup>
          </DataPageToolbar>
        }
      >
        <DataPageSurface className={logPanelVariants()}>
          {!following && !!logs?.length && (
            <LogFollowButton onClick={() => setFollowing(true)} />
          )}
          <RegisterContextMenu>
            <RegisterContextMenuTrigger asChild>
              <ScrollArea
                className="min-h-0 flex-1"
                data-slot="logs-scroll-area"
              >
                <Viewer
                  search={search}
                  following={following}
                  setFollowing={setFollowing}
                />
              </ScrollArea>
            </RegisterContextMenuTrigger>
            <RegisterContextMenuContent>
              <ContextMenuItem
                disabled={!logs?.length}
                onClick={handleClearLogs}
              >
                <DeleteForeverOutlineRounded aria-hidden className="size-4" />
                <span>{m.logs_action_clear_log()}</span>
              </ContextMenuItem>
            </RegisterContextMenuContent>
          </RegisterContextMenu>
        </DataPageSurface>
      </DataPage>
    </LogLevelsLayout>
  );
}
