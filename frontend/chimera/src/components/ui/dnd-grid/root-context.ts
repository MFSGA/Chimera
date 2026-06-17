/**
 * DnD Grid 根上下文
 *
 * 迁移自 ref: `src/components/ui/dnd-grid/root-context.ts`
 *
 * 提供 DndGridRoot 级别的上下文，用于：
 * - 注册/注销子 DndGrid（支持多 DndGrid 共存）
 * - 跟踪当前拖拽激活项的信息（用于 DragOverlay）
 *
 * 设计意图：
 * 当页面上有多个 DndGrid（例如 Dashboard 的"主网格"和 WidgetSheet 的"来源网格"）
 * 时，DndGridRoot 管理所有网格的注册，确保拖拽事件正确路由到对应的网格处理器。
 * 拖拽激活后，即使来源网格卸载，DndGridRoot 仍持有拖拽状态和 onSourceDrop 回调。
 */

import { createContext, useContext } from 'react';
import type { DragEndEvent, DragMoveEvent, DragStartEvent } from '@dnd-kit/core';

/**
 * 网格在 DndGridRoot 中的注册信息
 * 每个 DndGrid 实例通过 registerGrid 注册自己的回调
 */
export type GridRegistration = {
  itemIds: string[];
  dragIdPrefix: string;
  sourceOnly: boolean;
  handleDragStart: (e: DragStartEvent) => void;
  handleDragMove: (e: DragMoveEvent) => void;
  handleDragEnd: (e: DragEndEvent) => void;
  handleDragCancel: () => void;
  getCellSize: () => { cellW: number; cellH: number; gap: number };
  onSourceDrop?: (itemId: string) => void;
  onSourceDragStart?: () => void;
};

/** 当前拖拽激活项的尺寸信息（用于 DragOverlay） */
export type ActiveDrag = {
  itemId: string;
  dims: { width: number; height: number };
};

/** DndGridRoot 上下文值 */
export type DndGridRootContextValue = {
  registerGrid: (gridId: string, reg: GridRegistration) => void;
  unregisterGrid: (gridId: string) => void;
  activeDrag: ActiveDrag | null;
};

export const DndGridRootContext = createContext<DndGridRootContextValue | null>(
  null,
);

/**
 * 获取 DndGridRoot 上下文的 Hook
 * 返回 registerGrid、unregisterGrid 和 activeDrag
 */
export function useDndGridRoot() {
  return useContext(DndGridRootContext);
}
