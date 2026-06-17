/**
 * DnD Grid 布局计算 Hook
 *
 * 迁移自 ref: `src/components/ui/dnd-grid/use-grid-layout.ts`
 *
 * 职责：
 * - 使用 ResizeObserver 监听容器尺寸变化
 * - 计算网格的列数/行数、单元格像素尺寸
 * - 提供 getItemRect（项→像素坐标）和 snapToGrid（像素→网格对齐）函数
 *
 * 外部可通过 `size` 参数强制指定网格尺寸（相当于 CSS Grid 的 grid-template-columns），
 * 用于 WidgetSheet 等需要预览效果的场景。
 */

import { isEqual } from 'lodash-es';
import { useCallback, useEffect, useRef, useState } from 'react';
import type { DndGridItemType, GridLayout, GridSize, ItemRect } from './types';

/**
 * 网格布局 Hook
 *
 * @param minCellSize - 最小单元格像素尺寸
 * @param gap - 网格间距（像素）
 * @param size - 可选的外部指定网格尺寸（覆盖自动计算）
 */
export function useGridLayout(
  minCellSize: number,
  gap: number,
  size?: GridSize,
) {
  const containerRef = useRef<HTMLDivElement>(null);

  const [layout, setLayout] = useState<GridLayout>({
    cols: 1,
    rows: 1,
    cellW: minCellSize,
    cellH: minCellSize,
  });

  const [computedSize, setComputedSize] = useState<GridSize | null>(null);

  // 跟踪容器尺寸，以便在外部 `size` 变化时重新计算 cellW/cellH
  const containerSizeRef = useRef({ width: 0, height: 0 });
  const lastComputedSizeRef = useRef<GridSize | null>(null);

  useEffect(() => {
    const el = containerRef.current;
    if (!el) {
      return;
    }

    const recalculate = (width: number, height: number) => {
      if (width <= 0 || height <= 0) {
        return;
      }

      containerSizeRef.current = { width, height };

      // 计算可容纳的单元格数量：`n * size + (n-1) * gap <= total`
      // => n <= (total + gap) / (size + gap)
      const computedCols = Math.max(
        1,
        Math.floor((width + gap) / (minCellSize + gap)),
      );
      const computedRows = Math.max(
        1,
        Math.floor((height + gap) / (minCellSize + gap)),
      );

      const newComputedSize = {
        cols: computedCols,
        rows: computedRows,
      };

      // 避免重复触发不必要的 setState
      if (!isEqual(newComputedSize, lastComputedSizeRef.current)) {
        lastComputedSizeRef.current = newComputedSize;
        setComputedSize(newComputedSize);
      }

      // 如果外部指定了 size，使用外部的；否则使用自动计算的
      const cols = size?.cols ?? computedCols;
      const rows = size?.rows ?? computedRows;
      const cellW = (width - gap * (cols - 1)) / cols;
      const cellH = (height - gap * (rows - 1)) / rows;

      const nextLayout = { cols, rows, cellW, cellH };
      // 引用相等性检查，避免无限重渲染循环
      setLayout((prev) => (isEqual(prev, nextLayout) ? prev : nextLayout));
    };

    const observer = new ResizeObserver(([entry]) => {
      const { width, height } = entry.contentRect;
      recalculate(width, height);
    });

    observer.observe(el);

    // 初始计算
    const { width, height } = el.getBoundingClientRect();
    recalculate(width, height);

    return () => {
      observer.disconnect();
    };
  }, [minCellSize, gap, size?.cols, size?.rows]);

  /**
   * 根据网格项的位置尺寸计算其在容器中的像素矩形
   */
  const getItemRect = useCallback(
    (item: DndGridItemType): ItemRect => {
      const { cellW, cellH } = layout;
      return {
        left: item.x * (cellW + gap),
        top: item.y * (cellH + gap),
        width: item.w * cellW + (item.w - 1) * gap,
        height: item.h * cellH + (item.h - 1) * gap,
      };
    },
    [layout, gap],
  );

  /**
   * 将像素偏移转换为网格对齐后的新位置
   * 确保项不会超出网格边界
   */
  const snapToGrid = useCallback(
    <T extends string>(
      item: DndGridItemType<T>,
      deltaX: number,
      deltaY: number,
    ): DndGridItemType<T> => {
      const { cellW, cellH, cols, rows } = layout;
      const deltaCols = Math.round(deltaX / (cellW + gap));
      const deltaRows = Math.round(deltaY / (cellH + gap));

      return {
        ...item,
        x: Math.max(0, Math.min(cols - item.w, item.x + deltaCols)),
        y: Math.max(0, Math.min(rows - item.h, item.y + deltaRows)),
      };
    },
    [layout, gap],
  );

  return { containerRef, layout, computedSize, getItemRect, snapToGrid };
}
