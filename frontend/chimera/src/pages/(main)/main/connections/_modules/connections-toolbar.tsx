import type { TrafficRange } from '@chimera/interface';
import { cn } from '@chimera/ui';
import CloseRounded from '~icons/material-symbols/close-rounded';
import ViewColumnRounded from '~icons/material-symbols/view-column-rounded';
import type { ReactNode } from 'react';
import { Button } from '@/components/ui/button';
import {
  Tooltip,
  TooltipContent,
  TooltipTrigger,
} from '@/components/ui/tooltip';
import * as m from '@/paraglide/messages';
import type { SearchFilter } from '../../_modules/traffic-filters';
import ConnectionsFilters from './connections-filters';

export default function ConnectionsToolbar({
  start,
  tabs,
  selection,
  onSelectionChange,
  search,
  onSearchChange,
  onOpenSettings,
  onCloseAll,
}: {
  start?: ReactNode;
  tabs: ReactNode;
  selection: { range?: TrafficRange; filters: SearchFilter[] };
  onSelectionChange: (selection: {
    range?: TrafficRange;
    filters: SearchFilter[];
  }) => void;
  search: string;
  onSearchChange: (search: string) => void;
  onOpenSettings: () => void;
  onCloseAll?: () => void;
}) {
  const filtered =
    selection.range !== undefined || selection.filters.length > 0;

  return (
    <div
      className="bg-mixed-background @container shrink-0"
      data-slot="connections-toolbar"
    >
      <div
        className="flex min-h-16 flex-wrap items-center gap-3 px-4 py-3 @4xl:flex-nowrap @4xl:py-0"
        data-slot="connections-toolbar-content"
      >
        {start}
        {tabs}

        {filtered && (
          <ConnectionsFilters
            className="order-last basis-full @4xl:order-none @4xl:flex-1 @4xl:basis-0"
            selection={selection}
            onSelectionChange={onSelectionChange}
          />
        )}

        <input
          type="text"
          className={cn(
            'bg-surface-variant dark:bg-surface-variant/30',
            'h-10 min-w-48 flex-1 rounded-full px-4 text-sm outline-none',
            filtered && '@4xl:w-56 @4xl:flex-none @6xl:w-72',
          )}
          data-slot="connections-search"
          placeholder={m.connections_search_placeholder()}
          value={search}
          onChange={(event) => onSearchChange(event.target.value)}
        />

        <Tooltip>
          <TooltipTrigger asChild>
            <Button
              icon
              data-slot="connections-column-settings"
              onClick={onOpenSettings}
            >
              <ViewColumnRounded />
            </Button>
          </TooltipTrigger>
          <TooltipContent>{m.connections_column_settings()}</TooltipContent>
        </Tooltip>

        {onCloseAll && (
          <Tooltip>
            <TooltipTrigger asChild>
              <Button
                icon
                data-slot="connections-close-all"
                onClick={onCloseAll}
              >
                <CloseRounded />
              </Button>
            </TooltipTrigger>
            <TooltipContent>
              {m.connections_close_all_connections()}
            </TooltipContent>
          </Tooltip>
        )}
      </div>
    </div>
  );
}
