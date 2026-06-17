/**
 * DnD Grid 工具函数
 *
 * 迁移自 ref: `src/components/ui/dnd-grid/utils.ts`
 *
 * 提供网格布局相关的工具函数：
 * - isOverlap: 判断两个网格项是否重叠
 * - hasOverlap: 判断一个候选位置是否与现有项重叠（排除自身）
 * - calculateResize: 根据拖拽手柄方向计算调整大小后的新位置
 */

import type {
  DndGridItemType,
  GridItemConstraints,
  ResizeHandle,
} from './types';

/**
 * 判断两个网格项是否重叠（边界比较）
 * 使用 AABB（轴对齐边界框）碰撞检测
 */
export function isOverlap<T extends string>(
  a: DndGridItemType<T>,
  b: DndGridItemType<T>,
): boolean {
  return (
    a.x < b.x + b.w && a.x + a.w > b.x && a.y < b.y + b.h && a.y + a.h > b.y
  );
}

/**
 * 判断候选位置是否与现有项列表重叠（排除自身）
 * @param items - 现有项列表
 * @param movingId - 正在移动的项 ID（将被排除）
 * @param candidate - 候选位置
 * @returns 是否与任何其他项重叠
 */
export function hasOverlap<T extends string>(
  items: DndGridItemType<T>[],
  movingId: string,
  candidate: DndGridItemType<T>,
): boolean {
  return items.some(
    (item) => item.id !== movingId && isOverlap(candidate, item),
  );
}

/**
 * 根据拖拽手柄方向计算调整大小后的项位置
 *
 * 逻辑：
 * - 右侧手柄：增加/减少宽度（受 maxW/cols 约束）
 * - 底部手柄：增加/减少高度（受 maxH/rows 约束）
 * - 左侧手柄：调整 x 位置并反向调整宽度
 * - 顶部手柄：调整 y 位置并反向调整高度
 *
 * @param startItem - 调整前的项
 * @param handle - 拖拽手柄方向
 * @param deltaX - 水平像素偏移（屏幕坐标系）
 * @param deltaY - 垂直像素偏移（屏幕坐标系）
 * @param cellW - 单元格像素宽度
 * @param cellH - 单元格像素高度
 * @param gap - 网格间距
 * @param cols - 网格列数
 * @param rows - 网格行数
 * @param constraints - 项的尺寸约束
 */
export function calculateResize<T extends string>(
  startItem: DndGridItemType<T>,
  handle: ResizeHandle,
  deltaX: number,
  deltaY: number,
  cellW: number,
  cellH: number,
  gap: number,
  cols: number,
  rows: number,
  constraints: GridItemConstraints = {},
): DndGridItemType<T> {
  const minW = constraints.minW ?? 1;
  const minH = constraints.minH ?? 1;
  const maxW = constraints.maxW ?? cols;
  const maxH = constraints.maxH ?? rows;

  // 将像素偏移转换为网格单元偏移
  const stepX = cellW + gap;
  const stepY = cellH + gap;
  const deltaCols = Math.round(deltaX / stepX);
  const deltaRows = Math.round(deltaY / stepY);

  let { x, y, w, h } = startItem;

  // 右侧拖拽：调整宽度
  if (handle.includes('right')) {
    w = Math.max(minW, Math.min(maxW, cols - x, w + deltaCols));
  }

  // 底部拖拽：调整高度
  if (handle.includes('bottom')) {
    h = Math.max(minH, Math.min(maxH, rows - y, h + deltaRows));
  }

  // 左侧拖拽：调整 x 位置并反向调整宽度
  if (handle.includes('left')) {
    const newX = Math.max(0, Math.min(x + w - minW, x + deltaCols));
    const newW = Math.min(maxW, w + (x - newX));
    x = newX + (w + (x - newX) - newW);
    w = newW;
  }

  // 顶部拖拽：调整 y 位置并反向调整高度
  if (handle.includes('top')) {
    const newY = Math.max(0, Math.min(y + h - minH, y + deltaRows));
    const newH = Math.min(maxH, h + (y - newY));
    y = newY + (h + (y - newY) - newH);
    h = newH;
  }

  return {
    ...startItem,
    x,
    y,
    w,
    h,
  };
}
