import { Button } from '@chimera/ui/button';
import { DndGridSource } from '@chimera/ui/dnd-grid';
import { ScrollArea } from '@chimera/ui/scroll-area';
import { SearchField } from '@chimera/ui/search-field';
import { cn } from '@chimera/utils';
import AddRounded from '~icons/material-symbols/add-rounded';
import ArrowDownwardRounded from '~icons/material-symbols/arrow-downward-rounded';
import ArrowUpwardRounded from '~icons/material-symbols/arrow-upward-rounded';
import CloseRounded from '~icons/material-symbols/close-rounded';
import DoneRounded from '~icons/material-symbols/done-rounded';
import MemoryOutlineRounded from '~icons/material-symbols/memory-outline-rounded';
import SettingsEthernetRounded from '~icons/material-symbols/settings-ethernet-rounded';
import { cva } from 'class-variance-authority';
import { useState, type ComponentType } from 'react';
import { Drawer } from 'vaul';
import {
  RENDER_MAP,
  WIDGET_MIN_SIZE_MAP,
  WidgetId,
} from '@/components/widgets/consts';
import { useDashboardContext } from '@/components/widgets/provider';
import * as m from '@/paraglide/messages';

const WIDGET_TITLE_MAP: Record<WidgetId, () => string> = {
  [WidgetId.TrafficDown]: m.dashboard_widget_traffic_download,
  [WidgetId.TrafficUp]: m.dashboard_widget_traffic_upload,
  [WidgetId.Connections]: m.dashboard_widget_connections,
  [WidgetId.Memory]: m.dashboard_widget_memory,
  [WidgetId.ProxyShortcuts]: m.dashboard_widget_proxy_status,
  [WidgetId.CoreShortcuts]: m.dashboard_widget_core_status,
};

const WIDGET_ICON_MAP: Record<
  WidgetId,
  ComponentType<{ className?: string }>
> = {
  [WidgetId.TrafficDown]: ArrowDownwardRounded,
  [WidgetId.TrafficUp]: ArrowUpwardRounded,
  [WidgetId.Connections]: SettingsEthernetRounded,
  [WidgetId.Memory]: MemoryOutlineRounded,
  [WidgetId.ProxyShortcuts]: SettingsEthernetRounded,
  [WidgetId.CoreShortcuts]: MemoryOutlineRounded,
};

const widgetEntryAddVariants = cva('size-9 shrink-0 shadow-none', {
  variants: {
    placed: {
      true: 'bg-secondary-container text-on-secondary-container hover:bg-secondary-container',
      false: 'bg-surface-variant text-on-surface hover:bg-surface-variant',
    },
  },
});

function WidgetEntry({
  id,
  placed,
  onAdd,
  onDragStart,
}: {
  id: WidgetId;
  placed: boolean;
  onAdd: (id: WidgetId) => void;
  onDragStart: () => void;
}) {
  const title = WIDGET_TITLE_MAP[id]();
  const Icon = WIDGET_ICON_MAP[id];
  const { minW: w, minH: h } = WIDGET_MIN_SIZE_MAP[id];

  return (
    <DndGridSource
      itemId={id}
      dragIdPrefix="sheet:"
      w={w}
      h={h}
      onSourceDragStart={onDragStart}
      className="bg-surface-variant/40 flex min-w-0 items-center gap-3 rounded-2xl py-2 pr-2 pl-4"
      data-slot="widget-sheet-entry"
      data-widget-id={id}
      data-placed={placed || undefined}
    >
      <Icon className="text-on-surface-variant size-5 shrink-0" />

      <span className="min-w-0 flex-1 truncate text-sm">{title}</span>

      <span className="text-on-surface-variant text-xs tabular-nums">
        {w}×{h}
      </span>

      <Button
        variant="raised"
        icon
        className={widgetEntryAddVariants({ placed })}
        aria-label={m.dashboard_widget_library_add_entry({ name: title })}
        data-slot="widget-library-add"
        data-widget-id={id}
        onPointerDown={(event) => event.stopPropagation()}
        onTouchStart={(event) => event.stopPropagation()}
        onClick={() => onAdd(id)}
      >
        {placed ? (
          <DoneRounded className="size-5" />
        ) : (
          <AddRounded className="size-5" />
        )}
      </Button>
    </DndGridSource>
  );
}

export function WidgetSheet({
  placedTypes,
  onAdd,
}: {
  placedTypes: ReadonlySet<WidgetId>;
  onAdd: (id: WidgetId) => void;
}) {
  const { openSheet, setOpenSheet } = useDashboardContext();
  const [search, setSearch] = useState('');

  const query = search.trim().toLocaleLowerCase();
  const filteredIds = (Object.keys(RENDER_MAP) as WidgetId[]).filter(
    (id) =>
      !query || WIDGET_TITLE_MAP[id]().toLocaleLowerCase().includes(query),
  );

  const handleAdd = (id: WidgetId) => {
    onAdd(id);
    setOpenSheet(false);
  };

  return (
    <Drawer.Root open={openSheet} onOpenChange={setOpenSheet}>
      <Drawer.Portal>
        <Drawer.Overlay className="fixed inset-0 bg-black/30" />

        <Drawer.Content
          className={cn(
            'fixed inset-x-0 bottom-0 z-50 mx-auto w-[calc(100%-1rem)] max-w-[960px]',
            'dark:bg-surface/30 bg-surface-variant/30 backdrop-blur-3xl',
            'flex max-h-[85dvh] min-h-0 flex-col rounded-t-2xl',
            'dark:border-surface-variant/50 border-surface/50 border',
          )}
          aria-describedby={undefined}
        >
          <div className="shrink-0 px-4 pt-2 pb-3">
            <div className="flex items-center justify-between gap-4">
              <Drawer.Title className="text-lg font-semibold">
                {m.dashboard_add_widget()}
              </Drawer.Title>

              <Drawer.Close asChild>
                <Button
                  variant="raised"
                  className="size-8"
                  icon
                  aria-label={m.dashboard_widget_config_close_library()}
                >
                  <CloseRounded className="size-4" />
                </Button>
              </Drawer.Close>
            </div>

            <SearchField
              className="mt-3"
              value={search}
              onValueChange={setSearch}
              placeholder={m.dashboard_widget_library_search()}
              clearLabel={m.dashboard_widget_library_search_clear()}
            />
          </div>

          <ScrollArea className="min-h-0 flex-1">
            <div
              className="grid grid-cols-[repeat(auto-fill,minmax(13rem,1fr))] gap-2 px-4 pb-4"
              data-slot="widget-sheet-library"
            >
              {filteredIds.map((id) => (
                <WidgetEntry
                  key={id}
                  id={id}
                  placed={placedTypes.has(id)}
                  onAdd={handleAdd}
                  onDragStart={() => setOpenSheet(false)}
                />
              ))}

              {filteredIds.length === 0 && (
                <p
                  className="text-on-surface-variant col-span-full py-4 text-center text-sm"
                  data-slot="widget-library-empty"
                >
                  {m.dashboard_widget_library_no_results()}
                </p>
              )}
            </div>
          </ScrollArea>
        </Drawer.Content>
      </Drawer.Portal>
    </Drawer.Root>
  );
}
