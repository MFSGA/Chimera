import {
  useDeleteClashConnections,
  type TrafficRange,
  type TrafficScope,
} from '@chimera/interface';
import CloseRounded from '~icons/material-symbols/close-rounded';
import {
  useCallback,
  useDeferredValue,
  useMemo,
  useState,
  type ReactNode,
} from 'react';
import {
  RegisterContextMenu,
  RegisterContextMenuContent,
  RegisterContextMenuTrigger,
} from '@/components/providers/context-menu-provider';
import { ContextMenuItem } from '@/components/ui/context-menu';
import { ScrollArea } from '@/components/ui/scroll-area';
import { useLockFn } from '@/hooks/use-lock-fn';
import * as m from '@/paraglide/messages';
import type { SearchFilter } from '../../_modules/traffic-filters';
import { useSearchTerm } from '../../_modules/use-search-term';
import ActiveViewer from './active-viewer';
import AllViewer from './all-viewer';
import ClosedViewer from './closed-viewer';
import ConnectionsToolbar from './connections-toolbar';
import ConnectionsStatusTabs from './status-tabs';
import type { ConnectionDetail } from './table-row';
import { useTabFocus, type ConnectionsSelection } from './use-connection-rows';

/** Main URL-backed and Legacy local presentation selection share one view. */
export type ConnectionsViewState = {
  proxy?: string | null;
  scope: TrafficScope;
  range?: TrafficRange;
  filters: SearchFilter[];
  q?: string;
};

export default function ConnectionsContent({
  value,
  onChange,
  start,
  onLocateRule,
  onViewRuleUsage,
  entryFocus,
}: {
  value: ConnectionsViewState;
  onChange: (patch: Partial<ConnectionsViewState>) => void;
  start?: ReactNode;
  onLocateRule?: (detail: ConnectionDetail) => void;
  onViewRuleUsage?: (detail: ConnectionDetail) => void;
  entryFocus?: string;
}) {
  const { proxy, scope, range, filters, q } = value;
  const selection = useMemo<ConnectionsSelection>(
    () => ({ range, filters }),
    [range, filters],
  );
  const writeQuery = useCallback(
    (next: string | undefined) => onChange({ q: next }),
    [onChange],
  );
  const [search, setSearch] = useSearchTerm(q, writeQuery);
  const deferredSearch = useDeferredValue(search);
  const showTable = useDeferredValue(true, false);
  const [settingsOpen, setSettingsOpen] = useState(false);
  const focusRowId = useTabFocus(scope, entryFocus);
  const deleteConnections = useDeleteClashConnections();
  const handleCloseAllConnections = useLockFn(async () => {
    await deleteConnections.mutateAsync(null);
  });

  const scrollArea = (
    <ScrollArea
      key={scope}
      className="min-h-0 flex-1 [&>[data-slot=scroll-area-scrollbar][data-orientation=vertical]]:top-9! [&>[data-slot=scroll-area-scrollbar][data-orientation=vertical]]:h-auto"
      scrollbars="both"
      type="hover"
      data-slot="connections-scroll-wrapper"
    >
      {showTable &&
        (scope === 'all' ? (
          <AllViewer
            search={deferredSearch}
            proxy={proxy}
            selection={selection}
            settingsOpen={settingsOpen}
            onSettingsOpenChange={setSettingsOpen}
            onLocateRule={onLocateRule}
            onViewRuleUsage={onViewRuleUsage}
            focusRowId={focusRowId}
          />
        ) : scope === 'closed' ? (
          <ClosedViewer
            search={deferredSearch}
            proxy={proxy}
            selection={selection}
            settingsOpen={settingsOpen}
            onSettingsOpenChange={setSettingsOpen}
            onLocateRule={onLocateRule}
            onViewRuleUsage={onViewRuleUsage}
            focusRowId={focusRowId}
          />
        ) : (
          <ActiveViewer
            search={deferredSearch}
            proxy={proxy}
            filters={filters}
            settingsOpen={settingsOpen}
            onSettingsOpenChange={setSettingsOpen}
            onLocateRule={onLocateRule}
            onViewRuleUsage={onViewRuleUsage}
            focusRowId={focusRowId}
          />
        ))}
    </ScrollArea>
  );

  return (
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
        start={start}
        tabs={
          <ConnectionsStatusTabs
            value={scope}
            onValueChange={(next) => onChange({ scope: next })}
            selection={selection}
          />
        }
        selection={selection}
        onSelectionChange={(next) => onChange(next)}
        search={search}
        onSearchChange={setSearch}
        onOpenSettings={() => setSettingsOpen(true)}
        onCloseAll={scope === 'closed' ? undefined : handleCloseAllConnections}
      />
    </div>
  );
}
