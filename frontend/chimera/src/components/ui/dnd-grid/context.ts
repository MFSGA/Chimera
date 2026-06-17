/**
 * DnD Grid 上下文
 *
 * 迁移自 ref: `src/components/ui/dnd-grid/context.ts`
 *
 * 提供 DndGrid 上下文给子组件（DndGridItem），使其能够：
 * - 访问当前显示的网格项列表
 * - 获取项的像素坐标矩形
 * - 读取拖拽放置信息映射
 * - 检查当前激活/调整大小的项 ID
 * - 调用调整大小的回调函数
 * - 检查网格是否禁用/只读/覆盖层模式
 */

import { createContext, useContext, type RefObject } from 'react';
import type {
  DndGridItemType,
  GridItemConstraints,
  ItemRect,
  ResizeHandle,
} from './types';

/**
 * DnD Grid 上下文值类型
 * 包含 DndGridItem 渲染所需的所有状态和回调
 */
const DndGridContext = createContext<{
  displayItems: DndGridItemType[];
  getItemRect: (item: DndGridItemType) => ItemRect;
  dropInfoMap: Record<string, { left: number; top: number }>;
  activeItemId: string | null;
  resizingItemId: string | null;
  disabled: boolean;
  sourceOnly: boolean;
  dragIdPrefix: string;
  isOverlay: boolean;
  constraintsMapRef: RefObject<Record<string, GridItemConstraints>> & {
    current: Record<string, GridItemConstraints>;
  };
  onResizeStart: (
    id: string,
    handle: ResizeHandle,
    startX: number,
    startY: number,
  ) => void;
  onResizeMove: (currentX: number, currentY: number) => void;
  onResizeEnd: () => void;
} | null>(null);

export const DndGridProvider = DndGridContext.Provider;

/**
 * 获取 DndGrid 上下文的 Hook
 * 必须在 DndGrid 组件内部使用，否则会抛出错误
 */
export function useDndGridContext() {
  const ctx = useContext(DndGridContext);

  if (!ctx) {
    throw new Error('DndGridItem must be used inside DndGrid');
  }

  return ctx;
}
