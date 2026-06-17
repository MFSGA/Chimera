/**
 * DnD Grid 导出入口
 *
 * 迁移自 ref: `src/components/ui/dnd-grid/index.ts`
 *
 * 导出所有公共组件、Hook 和类型
 */

export { DndGrid, type DndGridProps } from './dnd-grid';
export { DndGridItem, type DndGridItemProps } from './dnd-grid-item';
export { DndGridProvider } from './context';
export { useDndGridRoot, type ActiveDrag } from './root-context';
export { DndGridRoot } from './dnd-grid-root';
export type {
  DndGridItemType,
  GridItemConstraints,
  GridSize,
  ResizeHandle,
} from './types';
