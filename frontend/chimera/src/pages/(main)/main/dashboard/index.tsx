import { Card, CardContent } from '@chimera/ui/card';
import { ContextMenuItem } from '@chimera/ui/context-menu';
import {
  DndGrid,
  DndGridProvider,
  DndGridRoot,
  useDndGridRoot,
  type DndGridItemType,
  type GridPosition,
  type GridSize,
} from '@chimera/ui/dnd-grid';
import { DragOverlay } from '@dnd-kit/core';
import { createFileRoute } from '@tanstack/react-router';
import AddRounded from '~icons/material-symbols/add-rounded';
import EditRounded from '~icons/material-symbols/edit-rounded';
import { useCallback, useEffect, useMemo, useRef, useState } from 'react';
import {
  RegisterContextMenu,
  RegisterContextMenuContent,
  RegisterContextMenuTrigger,
} from '@/components/providers/context-menu-provider';
import {
  DashboardItem,
  RENDER_MAP,
  WIDGET_MIN_SIZE_MAP,
  WIDGET_RECOMMENDED_SIZE_MAP,
  WidgetId,
} from '@/components/widgets/consts';
import { useDashboardContext } from '@/components/widgets/provider';
import { WidgetDataCalibration } from '@/components/widgets/widget-data-calibration';
import WidgetItem from '@/components/widgets/widget-item';
import { normalizeDashboardItems } from '@/components/widgets/widget-layout';
import { DashboardTrafficProvider } from '@/components/widgets/widget-traffic-provider';
import * as m from '@/paraglide/messages';
import EditAction from './_modules/edit-action';
import { adaptLayout } from './_modules/layout-adapt';
import {
  DashboardLayoutPersistenceProvider,
  useDashboardLayoutPersistence,
} from './_modules/layout-persistence';
import { selectLayoutEntry } from './_modules/layout-storage';
import { createDashboardViewportCoordinator } from './_modules/layout-viewport';
import { canEditDashboard, placeWidget } from './_modules/widget-placement';
import { WidgetSheet } from './_modules/widget-sheet';

export const Route = createFileRoute('/(main)/main/dashboard/')({
  component: RouteComponent,
});

// The layout picked for the grid's size stays while the window is being
// resized, so widgets that a smaller grid hides come back as it grows again.
// Once the size has settled this long, the layout is picked again for the
// new size, as it is when the page opens.
const SETTLED_RESIZE_MS = 200;

// Widgets declare these minimum sizes; they are known before any widget has
// rendered.
const constraintsFor = (items: DashboardItem[]) =>
  Object.fromEntries(
    items.map((item) => [item.id, WIDGET_MIN_SIZE_MAP[item.type]]),
  );

function DashboardDragOverlay({
  displayItems,
}: {
  displayItems: DashboardItem[];
}) {
  const root = useDndGridRoot();
  const activeDrag = root?.activeDrag ?? null;

  return (
    <DragOverlay dropAnimation={null}>
      {activeDrag &&
        (() => {
          const widgetType =
            displayItems.find((i) => i.id === activeDrag.itemId)?.type ??
            (activeDrag.itemId as WidgetId);
          const WidgetComponent = RENDER_MAP[widgetType];

          if (!WidgetComponent) {
            return null;
          }

          return (
            <div
              className="cursor-grabbing rounded-2xl opacity-90"
              style={{
                width: activeDrag.dims.width,
                height: activeDrag.dims.height,
              }}
            >
              <DndGridProvider
                value={{
                  displayItems: activeDrag.dragIdPrefix
                    ? []
                    : displayItems.filter(
                        (item) => item.id === activeDrag.itemId,
                      ),
                  getItemRect: () => ({
                    left: 0,
                    top: 0,
                    width: 0,
                    height: 0,
                  }),
                  dropInfoMap: {},
                  activeItemId: null,
                  resizingItemId: null,
                  disabled: true,
                  sourceOnly: true,
                  dragIdPrefix: activeDrag.dragIdPrefix,
                  isOverlay: true,
                  constraintsMapRef: { current: {} },
                  onResizeStart: () => {},
                  onResizeMove: () => {},
                  onResizeEnd: () => {},
                  onResizeCancel: () => {},
                }}
              >
                <WidgetComponent id={activeDrag.itemId} />
              </DndGridProvider>
            </div>
          );
        })()}
    </DragOverlay>
  );
}

const WidgetRender = () => {
  const { isEditing, configLoading, configReadError } = useDashboardContext();

  const {
    storage: layoutStorage,
    loading: layoutLoading,
    readError: layoutReadError,
    saveState: layoutSaveState,
    saveEdit: saveLayoutEdit,
    getSaveState,
  } = useDashboardLayoutPersistence();

  const editable = canEditDashboard({
    isEditing,
    layoutLoading,
    layoutReadError,
    layoutSaveBlocked: layoutSaveState !== 'saved',
    configLoading,
    configReadError,
  });

  const [gridSize, setGridSize] = useState<GridSize | null>(null);
  const [activeLayoutId, setActiveLayoutId] = useState<string | null>(null);
  const gridSizeRef = useRef<GridSize | null>(null);

  const layoutStorageRef = useRef(layoutStorage);
  layoutStorageRef.current = layoutStorage;

  const activeLayoutIdRef = useRef(activeLayoutId);
  activeLayoutIdRef.current = activeLayoutId;

  const sourceLayout = useMemo(() => {
    if (!gridSize) return null;

    return (
      layoutStorage.entries.find(({ id }) => id === activeLayoutId) ??
      selectLayoutEntry(layoutStorage, gridSize)
    );
  }, [layoutStorage, activeLayoutId, gridSize]);

  // Adapt from the pinned source each time; never persist this projection.
  const displayItems = useMemo(() => {
    if (!gridSize || !sourceLayout) return [];

    const items = normalizeDashboardItems(sourceLayout.items);
    return adaptLayout(items, gridSize, constraintsFor(items));
  }, [gridSize, sourceLayout]);

  const displayItemsRef = useRef(displayItems);
  displayItemsRef.current = displayItems;

  const placedTypes = useMemo(
    () => new Set(displayItems.map(({ type }) => type)),
    [displayItems],
  );

  const selectForSize = useCallback((size: GridSize) => {
    const id = selectLayoutEntry(layoutStorageRef.current, size)?.id ?? null;
    activeLayoutIdRef.current = id;
    setActiveLayoutId(id);
  }, []);

  const settledTimerRef = useRef<number | undefined>(undefined);
  useEffect(() => () => window.clearTimeout(settledTimerRef.current), []);

  const applySizeChange = useCallback(
    (newSize: GridSize) => {
      gridSizeRef.current = newSize;
      setGridSize(newSize);
      window.clearTimeout(settledTimerRef.current);
      if (activeLayoutIdRef.current === null) {
        selectForSize(newSize);
      } else {
        settledTimerRef.current = window.setTimeout(
          () => selectForSize(newSize),
          SETTLED_RESIZE_MS,
        );
      }
    },
    [selectForSize],
  );

  const viewport = useMemo(
    () => createDashboardViewportCoordinator(applySizeChange),
    [applySizeChange],
  );

  // A queue's state changes synchronously before the React commit. Check it
  // in each callback so a pending layout write also defers viewport changes.
  const handleSizeChange = useCallback(
    (size: GridSize) => {
      const saving = getSaveState() === 'saving';
      if (saving) window.clearTimeout(settledTimerRef.current);
      viewport.onSavingChange(saving);
      viewport.onSizeChange(size);
    },
    [viewport, getSaveState],
  );

  const handleGridInteractionChange = useCallback(
    (active: boolean) => {
      if (active) window.clearTimeout(settledTimerRef.current);
      const saving = getSaveState() === 'saving';
      if (saving) window.clearTimeout(settledTimerRef.current);
      viewport.onSavingChange(saving);
      viewport.onInteractionChange(active);
    },
    [viewport, getSaveState],
  );

  useEffect(() => {
    if (layoutSaveState === 'saving') {
      window.clearTimeout(settledTimerRef.current);
    }
    viewport.onSavingChange(layoutSaveState === 'saving');
  }, [viewport, layoutSaveState]);

  useEffect(() => {
    // A KV change event can arrive while dragging; don't reselect the
    // persisted source until the gesture or pending write has finished.
    if (viewport.isInteracting() || getSaveState() === 'saving') return;
    const available = gridSizeRef.current;
    const nextId =
      layoutStorage.preferredId ??
      (available ? selectLayoutEntry(layoutStorage, available)?.id : null) ??
      null;
    activeLayoutIdRef.current = nextId;
    setActiveLayoutId(nextId);
  }, [layoutStorage, layoutSaveState, viewport, getSaveState]);

  const handleLayoutChange = useCallback(
    (newItems: DashboardItem[]): boolean => {
      if (!editable || viewport.shouldDiscardCommit()) return false;

      const gridSize = gridSizeRef.current;
      const source =
        layoutStorageRef.current.entries.find(
          ({ id }) => id === activeLayoutIdRef.current,
        ) ??
        (gridSize
          ? selectLayoutEntry(layoutStorageRef.current, gridSize)
          : null);
      if (!source) return false;

      const saved = saveLayoutEdit({
        sourceId: source.id,
        visibleIds: displayItemsRef.current.map(({ id }) => id),
        nextItems: newItems,
      });
      if (!saved) return false;
      displayItemsRef.current = newItems;
      activeLayoutIdRef.current = source.id;
      setActiveLayoutId(source.id);
      return true;
    },
    [editable, saveLayoutEdit, viewport],
  );

  const handleCloseClick = useCallback(
    (id: string) =>
      handleLayoutChange(displayItemsRef.current.filter((i) => i.id !== id)),
    [handleLayoutChange],
  );

  const handleGridLayoutChange = useCallback(
    (newItems: DndGridItemType<string>[]) =>
      handleLayoutChange(normalizeDashboardItems(newItems)),
    [handleLayoutChange],
  );

  const renderWidget = useCallback(
    (item: DndGridItemType<string>) => {
      const type = (item as DashboardItem).type;
      const WidgetComponent = RENDER_MAP[type];

      if (configLoading || configReadError) {
        return (
          <WidgetItem
            id={item.id}
            widgetType={type}
            {...WIDGET_MIN_SIZE_MAP[type]}
            onCloseClick={handleCloseClick}
          >
            <Card className="size-full">
              <CardContent
                className="size-full justify-center text-sm"
                role="status"
              >
                {configReadError
                  ? m.dashboard_widget_config_load_failed()
                  : m.dashboard_widget_config_loading()}
              </CardContent>
            </Card>
          </WidgetItem>
        );
      }

      return <WidgetComponent id={item.id} onCloseClick={handleCloseClick} />;
    },
    [handleCloseClick, configLoading, configReadError],
  );

  const addWidgetFromSheet = useCallback(
    (widgetId: WidgetId, position?: GridPosition): boolean => {
      if (!editable) return false;

      const gridSize = gridSizeRef.current;
      if (!gridSize) return false;

      const { cols, rows } = gridSize;
      const current = displayItemsRef.current;
      const instanceId = crypto.randomUUID();
      const placement = placeWidget({
        id: instanceId,
        type: widgetId,
        items: current,
        cols,
        rows,
        minimum: WIDGET_MIN_SIZE_MAP[widgetId],
        recommended: WIDGET_RECOMMENDED_SIZE_MAP[widgetId],
        position,
      });

      if (placement) {
        return handleLayoutChange([...current, placement]);
      }
      return false;
    },
    [handleLayoutChange, editable],
  );

  return (
    <DndGridRoot onSourceInteractionChange={handleGridInteractionChange}>
      <div
        className="flex min-h-0 flex-1 flex-col p-4"
        data-slot="dashboard-widget-container"
      >
        <DashboardTrafficProvider items={displayItems}>
          <WidgetDataCalibration items={displayItems} />

          <DndGrid
            gridId="main"
            className="min-h-0 flex-1"
            items={displayItems}
            onLayoutChange={handleGridLayoutChange}
            onExternalDrop={(id, position) =>
              addWidgetFromSheet(id as WidgetId, position)
            }
            minCellSize={64}
            onSizeChange={handleSizeChange}
            onInteractionChange={handleGridInteractionChange}
            gap={16}
            disabled={!editable}
          >
            {renderWidget}
          </DndGrid>
        </DashboardTrafficProvider>
      </div>

      <DashboardDragOverlay displayItems={displayItems} />

      <WidgetSheet
        placedTypes={placedTypes}
        onAdd={addWidgetFromSheet}
        disabled={!editable}
      />
    </DndGridRoot>
  );
};

function RouteComponent() {
  const { setIsEditing, setOpenSheet } = useDashboardContext();

  return (
    <RegisterContextMenu>
      <RegisterContextMenuTrigger asChild>
        <div
          data-slot="dashboard-container"
          className="relative flex min-h-0 flex-1 flex-col overflow-hidden"
        >
          <DashboardLayoutPersistenceProvider>
            <WidgetRender />
            <EditAction />
          </DashboardLayoutPersistenceProvider>
        </div>
      </RegisterContextMenuTrigger>

      <RegisterContextMenuContent>
        <ContextMenuItem
          data-slot="dashboard-context-menu-edit-widgets"
          onSelect={() => setIsEditing(true)}
        >
          <EditRounded className="size-4" />

          <span>{m.dashboard_context_menu_edit_widgets()}</span>
        </ContextMenuItem>

        <ContextMenuItem
          onSelect={() => {
            setIsEditing(true);
            setOpenSheet(true);
          }}
        >
          <AddRounded className="size-4" />

          <span>{m.dashboard_context_menu_add_widgets()}</span>
        </ContextMenuItem>
      </RegisterContextMenuContent>
    </RegisterContextMenu>
  );
}
