import {
  useDeleteClashConnections,
  type TrafficRange,
  type TrafficScope,
} from '@chimera/interface';
import { ContextMenuItem } from '@chimera/ui/context-menu';
import { Modal } from '@chimera/ui/modal';
import { ScrollArea } from '@chimera/ui/scroll-area';
import { cn } from '@chimera/utils';
import CloseRounded from '~icons/material-symbols/close-rounded';
import { useCallback, useDeferredValue, useMemo, type ReactNode } from 'react';
import {
  RegisterContextMenu,
  RegisterContextMenuContent,
  RegisterContextMenuTrigger,
} from '@/components/providers/context-menu-provider';
import { useLockFn } from '@/hooks/use-lock-fn';
import * as m from '@/paraglide/messages';
import { DataPage, DataPageSurface } from '../../_modules/data-page';
import type { SearchFilter } from '../../_modules/traffic-filters';
import { useSearchTerm } from '../../_modules/use-search-term';
import ActiveViewer from './active-viewer';
import AllViewer from './all-viewer';
import ClosedViewer from './closed-viewer';
import ConnectionsToolbar from './connections-toolbar';
import ConnectionsToolbarRef from './connections-toolbar-ref';
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
  openDetailsOnClick = false,
  referenceLayout = false,
}: {
  value: ConnectionsViewState;
  onChange: (patch: Partial<ConnectionsViewState>) => void;
  start?: ReactNode;
  onLocateRule?: (detail: ConnectionDetail) => void;
  onViewRuleUsage?: (detail: ConnectionDetail) => void;
  entryFocus?: string;
  /** Legacy presentation can open the shared detail modal with one click. */
  openDetailsOnClick?: boolean;
  /** Ref DataPage bottom dock only for the main route. */
  referenceLayout?: boolean;
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
  const focusRowId = useTabFocus(scope, entryFocus);
  const deleteConnections = useDeleteClashConnections();
  const handleCloseAllConnections = useLockFn(async () => {
    await deleteConnections.mutateAsync(null);
  });

  const scrollArea = (
    <ScrollArea
      key={scope}
      className={cn(
        'min-h-0 flex-1 [&>[data-slot=scroll-area-scrollbar][data-orientation=vertical]]:h-auto',
        referenceLayout
          ? '[&>[data-slot=scroll-area-scrollbar][data-orientation=vertical]]:top-10!'
          : '[&>[data-slot=scroll-area-scrollbar][data-orientation=vertical]]:top-9!',
      )}
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
            onLocateRule={onLocateRule}
            onViewRuleUsage={onViewRuleUsage}
            focusRowId={focusRowId}
            openDetailsOnClick={openDetailsOnClick}
          />
        ) : scope === 'closed' ? (
          <ClosedViewer
            search={deferredSearch}
            proxy={proxy}
            selection={selection}
            onLocateRule={onLocateRule}
            onViewRuleUsage={onViewRuleUsage}
            focusRowId={focusRowId}
            openDetailsOnClick={openDetailsOnClick}
          />
        ) : (
          <ActiveViewer
            search={deferredSearch}
            proxy={proxy}
            filters={filters}
            onLocateRule={onLocateRule}
            onViewRuleUsage={onViewRuleUsage}
            focusRowId={focusRowId}
            openDetailsOnClick={openDetailsOnClick}
          />
        ))}
    </ScrollArea>
  );

  const connectionList =
    scope === 'closed' ? (
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
    );

  const toolbarProps = {
    start,
    tabs: (
      <ConnectionsStatusTabs
        value={scope}
        onValueChange={(next) => onChange({ scope: next })}
        selection={selection}
      />
    ),
    selection,
    onSelectionChange: (next: ConnectionsSelection) => onChange(next),
    search,
    onSearchChange: setSearch,
    onCloseAll: scope === 'closed' ? undefined : handleCloseAllConnections,
  };

  return (
    <Modal>
      {referenceLayout ? (
        <DataPage
          title={m.navbar_label_connections()}
          toolbar={<ConnectionsToolbarRef {...toolbarProps} />}
        >
          <DataPageSurface data-connections-container="true">
            {connectionList}
          </DataPageSurface>
        </DataPage>
      ) : (
        <div
          className="divide-outline-variant flex min-h-0 flex-1 flex-col divide-y overflow-hidden"
          data-slot="connections-container"
        >
          {connectionList}
          <ConnectionsToolbar {...toolbarProps} />
        </div>
      )}
    </Modal>
  );
}
