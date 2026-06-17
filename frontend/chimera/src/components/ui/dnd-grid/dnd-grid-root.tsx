/**
 * DnD Grid 根组件
 *
 * 迁移自 ref: `src/components/ui/dnd-grid/dnd-grid-root.tsx`
 *
 * 职责：
 * - 包装在 DnD 页面最外层（如 Dashboard）
 * - 提供 DndContext 和根级上下文
 * - 管理多个 DndGrid 子实例的注册
 * - 处理拖拽事件路由：将 @dnd-kit 事件分发到对应网格的注册处理器
 *
 * 多网格支持：
 * 当页面上有多个 DndGrid（如 Dashboard 的主网格 + WidgetSheet 的来源网格），
 * DndGridRoot 通过 findGrid 查找拖拽项所属的网格，将事件路由到该网格的处理器。
 * 即使来源网格在拖拽中卸载（WidgetSheet 关闭），pendingSourceRef 仍保留 onSourceDrop
 * 回调以完成拖拽放置。
 */

import { useCallback, useRef, useState, type PropsWithChildren } from 'react';
import {
  DndContext,
  PointerSensor,
  TouchSensor,
  useSensor,
  useSensors,
  type DragEndEvent,
  type DragMoveEvent,
  type DragStartEvent,
} from '@dnd-kit/core';
import {
  DndGridRootContext,
  type ActiveDrag,
  type GridRegistration,
} from './root-context';

/**
 * 根据拖拽项的 ID 查找其所属的网格注册信息
 * 遍历所有已注册的网格，检查项的 ID 是否匹配（考虑 dragIdPrefix）
 */
function findGrid(
  grids: Map<string, GridRegistration>,
  activeId: string,
): { gridId: string; reg: GridRegistration; plainId: string } | null {
  for (const [gridId, reg] of grids) {
    const { itemIds, dragIdPrefix } = reg;

    if (dragIdPrefix) {
      if (activeId.startsWith(dragIdPrefix)) {
        const plain = activeId.slice(dragIdPrefix.length);

        if (itemIds.includes(plain)) {
          return {
            gridId,
            reg,
            plainId: plain,
          };
        }
      }
    } else if (itemIds.includes(activeId)) {
      return {
        gridId,
        reg,
        plainId: activeId,
      };
    }
  }

  return null;
}

/**
 * DndGridRoot 组件
 *
 * 用法：
 * ```tsx
 * <DndGridRoot>
 *   <DndGrid gridId="main" ...>
 *     {renderWidgets}
 *   </DndGrid>
 *   <WidgetSheet />
 * </DndGridRoot>
 * ```
 */
export function DndGridRoot({ children }: PropsWithChildren) {
  const gridsRef = useRef<Map<string, GridRegistration>>(new Map());

  const [activeDrag, setActiveDrag] = useState<ActiveDrag | null>(null);

  // 拖拽开始时捕获 onSourceDrop，以应对来源网格在拖拽中卸载（例如 WidgetSheet 关闭）
  const pendingSourceRef = useRef<{
    onSourceDrop?: (itemId: string) => void;
    itemId: string;
  } | null>(null);

  const registerGrid = useCallback((gridId: string, reg: GridRegistration) => {
    gridsRef.current.set(gridId, reg);
  }, []);

  const unregisterGrid = useCallback((gridId: string) => {
    gridsRef.current.delete(gridId);
  }, []);

  // 触控和鼠标传感器配置
  const sensors = useSensors(
    useSensor(PointerSensor, { activationConstraint: { distance: 6 } }),
    useSensor(TouchSensor, {
      activationConstraint: { delay: 200, tolerance: 6 },
    }),
  );

  /**
   * 拖拽开始处理：
   * 1. 查找项所属的网格
   * 2. 如果是来源网格（sourceOnly），捕获 onSourceDrop 并触发 onSourceDragStart
   * 3. 否则委派到网格自身的 handleDragStart
   * 4. 设置 activeDrag 用于 DragOverlay
   */
  const handleDragStart = useCallback((e: DragStartEvent) => {
    const activeId = String(e.active.id);
    const found = findGrid(gridsRef.current, activeId);

    if (!found) {
      return;
    }

    const { reg, plainId } = found;
    const { sourceOnly, getCellSize, onSourceDragStart } = reg;

    const data = e.active.data.current as { w?: number; h?: number } | undefined;
    const { cellW, cellH, gap } = getCellSize();
    const w = data?.w ?? 2;
    const h = data?.h ?? 2;

    if (sourceOnly) {
      // 在 onSourceDragStart 可能卸载网格之前捕获回调
      pendingSourceRef.current = {
        onSourceDrop: reg.onSourceDrop,
        itemId: plainId,
      };
      onSourceDragStart?.();
    } else {
      reg.handleDragStart(e);
    }

    setActiveDrag({
      itemId: plainId,
      dims: {
        width: w * cellW + (w - 1) * gap,
        height: h * cellH + (h - 1) * gap,
      },
    });
  }, []);

  /**
   * 拖拽移动处理：
   * 非 sourceOnly 的网格才需要实时更新预览位置
   */
  const handleDragMove = useCallback((e: DragMoveEvent) => {
    const activeId = String(e.active.id);
    const found = findGrid(gridsRef.current, activeId);

    if (!found) {
      return;
    }

    const { reg } = found;

    if (!reg.sourceOnly) {
      reg.handleDragMove(e);
    }
  }, []);

  /**
   * 拖拽结束处理：
   * 1. 如果有 pending source drop，调用 onSourceDrop 并返回
   * 2. 否则委派到网格自身的 handleDragEnd
   */
  const handleDragEnd = useCallback((e: DragEndEvent) => {
    setActiveDrag(null);

    // 来源拖拽：使用捕获的处理器（网格可能已经卸载）
    if (pendingSourceRef.current) {
      const { onSourceDrop, itemId } = pendingSourceRef.current;
      pendingSourceRef.current = null;
      onSourceDrop?.(itemId);
      return;
    }

    const activeId = String(e.active.id);
    const found = findGrid(gridsRef.current, activeId);

    if (!found) {
      return;
    }

    found.reg.handleDragEnd(e);
  }, []);

  /**
   * 拖拽取消处理：
   * 清空所有状态并通知所有非 sourceOnly 的网格
   */
  const handleDragCancel = useCallback(() => {
    pendingSourceRef.current = null;
    setActiveDrag(null);

    for (const [, reg] of gridsRef.current) {
      if (!reg.sourceOnly) {
        reg.handleDragCancel();
      }
    }
  }, []);

  return (
    <DndGridRootContext.Provider
      value={{ registerGrid, unregisterGrid, activeDrag }}
    >
      <DndContext
        sensors={sensors}
        onDragStart={handleDragStart}
        onDragMove={handleDragMove}
        onDragEnd={handleDragEnd}
        onDragCancel={handleDragCancel}
      >
        {children}
      </DndContext>
    </DndGridRootContext.Provider>
  );
}
