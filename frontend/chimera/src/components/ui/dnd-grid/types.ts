/**
 * DnD Grid 类型定义
 *
 * 迁移自 ref: `src/components/ui/dnd-grid/types.ts`
 *
 * 定义拖拽网格系统的核心类型：
 * - DndGridItemType: 网格中每个项的位置和尺寸
 * - GridItemConstraints: 项的最小/最大尺寸约束
 * - GridSize: 网格的列数和行数
 * - GridLayout: 网格的完整布局（含单元格尺寸）
 * - ItemRect: 项的像素坐标矩形
 * - ResizeHandle: 调整大小的拖拽手柄方向
 */

export type ResizeHandle =
  | 'top'
  | 'top-right'
  | 'right'
  | 'bottom-right'
  | 'bottom'
  | 'bottom-left'
  | 'left'
  | 'top-left';

/**
 * DnD 网格项的通用类型
 * @typeParam T - 项 ID 的类型（默认 string）
 */
export type DndGridItemType<T = string> = {
  id: T;
  /** 列起始索引（从 0 开始） */
  x: number;
  /** 行起始索引（从 0 开始） */
  y: number;
  /** 宽度（占网格单元数） */
  w: number;
  /** 高度（占网格单元数） */
  h: number;
};

/** 网格项的尺寸约束 */
export type GridItemConstraints = {
  /** 最小宽度（网格单元数，默认 1） */
  minW?: number;
  /** 最小高度（网格单元数，默认 1） */
  minH?: number;
  /** 最大宽度（网格单元数） */
  maxW?: number;
  /** 最大高度（网格单元数） */
  maxH?: number;
};

/** 网格尺寸：列数和行数 */
export interface GridSize {
  cols: number;
  rows: number;
}

/** 网格完整布局，包含计算后的单元格像素尺寸 */
export interface GridLayout extends GridSize {
  cellW: number;
  cellH: number;
}

/** 项的像素坐标矩形 */
export interface ItemRect {
  left: number;
  top: number;
  width: number;
  height: number;
}
