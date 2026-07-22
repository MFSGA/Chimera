/**
 * Dashboard 主页（Widget 网格系统）
 *
 * 迁移自 ref: `src/pages/(main)/main/dashboard/index.tsx`
 *
 * 职责：
 * - 展示可拖拽、可调整大小的 Dashboard DnD widget 网格
 * - 支持编辑模式：添加 widget、调整大小、删除 widget、拖拽重排
 * - 布局持久化：通过 useKvStorage 保存用户自定义布局
 * - 响应式自适应：当窗口尺寸变化时，自动切换最匹配的预设布局
 * - 提供 WidgetSheet（底部抽屉）让用户添加新的 widget
 * - 支持从 WidgetSheet 拖拽 widget 到主网格
 *
 * 核心组件：
 * - WidgetRender：管理 DndGrid 布局状态，处理尺寸变化、布局持久化、widget 添加
 * - DashboardDragOverlay：拖拽时的 DragOverlay 预览
 * - EditAction：编辑模式浮动操作栏
 * - WidgetSheet：底部抽屉，显示所有可用 widget
 *
 * 布局自适应流程：
 * 1. 窗口尺寸变化 → DndGrid onSizeChange
 * 2. findBestLayout 查找最匹配的预设布局
 * 3. 如果无匹配 → findClosestStoredLayout + adaptLayout 自适应
 * 4. 用户拖拽/调整后 → handleLayoutChange 持久化到 KvStorage
 *
 * 适配说明：
 * - 使用 @dnd-kit/core 的 DragOverlay 替代 ref 中的嵌套（已统一到 DndGridRoot）
 * - 使用 useKvStorage 持久化布局（与 Chimera 的 KV 存储兼容）
 * - 使用与 ref 一致的 context-menu 进入编辑和添加组件状态
 */

import { useKvStorage } from '@chimera/interface';
import { DragOverlay } from '@dnd-kit/core';
import { createFileRoute } from '@tanstack/react-router';
import AddRounded from '~icons/material-symbols/add-rounded';
import EditRounded from '~icons/material-symbols/edit-rounded';
import { useCallback, useRef, useState } from 'react';
import {
  RegisterContextMenu,
  RegisterContextMenuContent,
  RegisterContextMenuTrigger,
} from '@/components/providers/context-menu-provider';
import { ContextMenuItem } from '@/components/ui/context-menu';
import {
  DndGrid,
  DndGridProvider,
  DndGridRoot,
  useDndGridRoot,
  type DndGridItemType,
  type GridItemConstraints,
  type GridSize,
} from '@/components/ui/dnd-grid';
import { hasOverlap } from '@/components/ui/dnd-grid/utils';
import * as m from '@/paraglide/messages';
import {
  DashboardItem,
  DEFAULT_ITEMS,
  DEFAULT_LAYOUTS,
  LayoutStorage,
  RENDER_MAP,
  WIDGET_MIN_SIZE_MAP,
  WidgetId,
} from './_modules/consts';
import EditAction from './_modules/edit-action';
import {
  adaptLayout,
  findBestLayout,
  findClosestStoredLayout,
  sizeKey,
} from './_modules/layout-adapt';
import { useDashboardContext } from './_modules/provider';
import { WidgetSheet } from './_modules/widget-sheet';

/**
 * 注册 TanStack Router 文件路由
 * 路径: `/main/dashboard/`（index 子路由）
 * 匹配 ref: `createFileRoute('/(main)/main/dashboard/')`
 */
export const Route = createFileRoute('/(main)/main/dashboard/')({
  component: RouteComponent,
});

/**
 * 标准化网格项：确保每个项都有 type 属性
 * 从存储中恢复的项可能缺少 type 字段（旧格式兼容）
 */
function normalizeItems(items: DndGridItemType<string>[]): DashboardItem[] {
  return items.map((item) => ({
    ...item,
    type: (item as DashboardItem).type ?? (item.id as WidgetId),
  }));
}

/**
 * DashboardDragOverlay — 拖拽时的浮动预览
 *
 * 在 DndGridRoot 上下文中读取 activeDrag 信息，
 * 渲染被拖拽 widget 的预览（包裹在 WidgetComponent 中）。
 *
 * 使用 DndGridProvider 子上下文确保 overlay 内的 WidgetComponent
 * 能正确获取 isOverlay=true 状态（跳过定位逻辑）。
 */
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
                  displayItems: [],
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
                  dragIdPrefix: '',
                  isOverlay: true,
                  constraintsMapRef: { current: {} },
                  onResizeStart: () => {},
                  onResizeMove: () => {},
                  onResizeEnd: () => {},
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

/**
 * WidgetRender — 核心 widget 网格渲染器
 *
 * 管理 DndGrid 的布局状态，包括：
 * - 网格尺寸变化时的布局自适应（findBestLayout → adaptLayout）
 * - 用户拖拽/调整大小后的布局持久化（useKvStorage）
 * - 从 WidgetSheet 添加新 widget（addWidgetFromSheet）
 *
 * 使用 ref 跟踪最新状态，避免闭包过期问题。
 */
const WidgetRender = () => {
  const { isEditing, setOpenSheet } = useDashboardContext();

  // 持久化布局存储（从 KvStorage 读取/写入）
  const [layoutStorage, setLayoutStorage] = useKvStorage<LayoutStorage>(
    'dashboard-widgets',
    DEFAULT_LAYOUTS,
  );

  // 当前显示的 widget 项列表
  const [displayItems, setDisplayItems] =
    useState<DashboardItem[]>(DEFAULT_ITEMS);

  // 使用 ref 避免闭包过期
  const layoutStorageRef = useRef(layoutStorage);
  layoutStorageRef.current = layoutStorage;

  const displayItemsRef = useRef(displayItems);
  displayItemsRef.current = displayItems;

  const gridSizeRef = useRef<GridSize>({ cols: 1, rows: 1 });

  /**
   * 网格尺寸变化处理
   *
   * 逻辑：
   * 1. 先在预设布局存储中查找完全匹配的布局
   * 2. 如果有，直接使用
   * 3. 如果没有，查找最接近的预设布局 + adaptLayout 自适应
   */
  const handleSizeChange = useCallback(
    (
      newSize: GridSize,
      constraintsMap: Record<string, GridItemConstraints>,
    ) => {
      gridSizeRef.current = newSize;

      // 尝试找到完全匹配的预设布局
      const bestLayout = findBestLayout(layoutStorageRef.current, newSize);

      if (bestLayout) {
        const normalized = normalizeItems(bestLayout);
        displayItemsRef.current = normalized;
        setDisplayItems(normalized);
        return;
      }

      // 无完全匹配 → 找最接近的 + 自适应
      const base =
        findClosestStoredLayout(layoutStorageRef.current, newSize) ??
        DEFAULT_ITEMS;

      const nextItems = normalizeItems(
        adaptLayout(base, newSize, constraintsMap),
      );
      displayItemsRef.current = nextItems;
      setDisplayItems(nextItems);
    },
    [],
  );

  /**
   * 布局变化处理（拖拽/调整大小后）
   * 更新当前显示状态并持久化到 KvStorage
   */
  const handleLayoutChange = useCallback(
    (newItems: DashboardItem[]) => {
      const key = sizeKey(gridSizeRef.current);
      displayItemsRef.current = newItems;
      setDisplayItems(newItems);

      // 更新并持久化布局
      layoutStorageRef.current = {
        ...layoutStorageRef.current,
        [key]: newItems,
      };
      setLayoutStorage(layoutStorageRef.current);
    },
    [setLayoutStorage],
  );

  /**
   * 从 WidgetSheet 添加新 widget
   *
   * 逻辑：
   * 1. 使用 WIDGET_MIN_SIZE_MAP 获取 widget 的最小尺寸
   * 2. 在网格中从左上到右下扫描空闲位置
   * 3. 如果找到空闲位置，放置在那里
   * 4. 如果无空闲位置，放置在最后一行的下一个可用行
   */
  const addWidgetFromSheet = useCallback(
    (widgetId: WidgetId) => {
      const { minW, minH } = WIDGET_MIN_SIZE_MAP[widgetId];
      const { cols, rows } = gridSizeRef.current;
      const current = displayItemsRef.current;
      const instanceId = crypto.randomUUID();

      // 扫描空闲位置
      const findPlacement = (): DashboardItem => {
        for (let y = 0; y <= rows; y++) {
          for (let x = 0; x <= cols - minW; x++) {
            const candidate: DashboardItem = {
              id: instanceId,
              type: widgetId,
              x,
              y,
              w: minW,
              h: minH,
            };

            if (!hasOverlap(current, instanceId, candidate)) {
              return candidate;
            }
          }
        }

        // 无空闲位置 → 追加到最底部
        const maxY = current.reduce((m, i) => Math.max(m, i.y + i.h), 0);
        return {
          id: instanceId,
          type: widgetId,
          x: 0,
          y: maxY,
          w: minW,
          h: minH,
        };
      };

      handleLayoutChange([...current, findPlacement()]);
    },
    [handleLayoutChange],
  );

  return (
    <DndGridRoot>
      <div
        className="flex min-h-0 flex-1 flex-col"
        data-slot="dashboard-widget-container"
      >
        {/* DnD Widget 网格 */}
        <DndGrid
          gridId="main"
          className="min-h-0 flex-1 px-4"
          items={displayItems}
          onLayoutChange={(newItems) =>
            handleLayoutChange(normalizeItems(newItems))
          }
          minCellSize={64}
          onSizeChange={handleSizeChange}
          gap={16}
          disabled={!isEditing}
        >
          {(item) => {
            const WidgetComponent = RENDER_MAP[(item as DashboardItem).type];

            return (
              <WidgetComponent
                id={item.id}
                onCloseClick={() =>
                  handleLayoutChange(
                    displayItemsRef.current.filter((i) => i.id !== item.id),
                  )
                }
              />
            );
          }}
        </DndGrid>
      </div>

      {/* 拖拽浮动预览 */}
      <DashboardDragOverlay displayItems={displayItemsRef.current} />

      {/* Widget 添加面板 */}
      <WidgetSheet
        onSourceDrop={(id) => addWidgetFromSheet(id)}
        onSourceDragStart={() => setOpenSheet(false)}
      />
    </DndGridRoot>
  );
};

/**
 * Dashboard 页面主组件
 *
 * 布局结构（匹配 ref）：
 * - 编辑按钮（右上角）
 * - WidgetRender（DndGrid 网格系统）
 * - EditAction（编辑模式浮动栏）
 *
 * 使用 DndGridRoot 管理多网格拖拽事件路由，
 * EditAction 在编辑模式时显示浮动操作栏。
 */
function RouteComponent() {
  const { setIsEditing, setOpenSheet } = useDashboardContext();

  return (
    <RegisterContextMenu>
      <RegisterContextMenuTrigger asChild>
        <div
          data-slot="dashboard-container"
          className="relative flex min-h-0 flex-1 flex-col overflow-hidden"
        >
          <WidgetRender />

          <EditAction />
        </div>
      </RegisterContextMenuTrigger>

      <RegisterContextMenuContent>
        <ContextMenuItem onSelect={() => setIsEditing(true)}>
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
