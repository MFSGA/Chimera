/**
 * Dashboard 布局自适应工具函数
 *
 * 迁移自 ref: `src/pages/(main)/main/dashboard/_modules/layout-adapt.ts`
 *
 * 职责：
 * - 当窗口/网格尺寸变化时，从预设布局中找到最匹配的布局
 * - 如果没有任何预设布局完全匹配，提供布局自适应算法
 * - 自适应算法：按优先级 clamp → 找空闲位置 → 缩小 → 丢弃
 *
 * 布局适应逻辑（adaptLayout）：
 * 1. 按阅读顺序（自上而下，自左而右）处理每个项
 * 2. 先尝试 clamp 到边界内，若无重叠则保留
 * 3. 若重叠，在当前尺寸下扫描空闲位置
 * 4. 若仍不可放置，逐步缩小尺寸（向 minW/minH 方向）
 * 5. 若最小尺寸也无法放置，丢弃该项
 */

import type {
  DndGridItemType,
  GridItemConstraints,
  GridSize,
} from '@/components/ui/dnd-grid';
import { isOverlap } from '@/components/ui/dnd-grid/utils';

/**
 * 将 GridSize 转换为存储键名 `{cols}x{rows}`
 */
export function sizeKey(size: GridSize): string {
  return `${size.cols}x${size.rows}`;
}

/**
 * 在预设布局存储中找到最佳匹配项
 *
 * 扫描所有尺寸 ≤ 当前尺寸的预设布局，返回面积最大的那个。
 * 例如当前是 14x6，返回 12x6 的布局（如果存在）。
 *
 * @param storage - 预设布局存储
 * @param size - 当前网格尺寸
 * @returns 匹配的布局项，或 null
 */
export function findBestLayout<T extends DndGridItemType<string>>(
  storage: Record<string, T[]>,
  size: GridSize,
): T[] | null {
  let best: { area: number; items: T[] } | null = null;

  for (const [key, items] of Object.entries(storage)) {
    const match = key.match(/^(\d+)x(\d+)$/);
    if (!match) continue;

    const cols = parseInt(match[1], 10);
    const rows = parseInt(match[2], 10);

    if (cols <= size.cols && rows <= size.rows) {
      const area = cols * rows;

      if (!best || area > best.area) {
        best = { area, items };
      }
    }
  }

  return best?.items ?? null;
}

/**
 * 当没有任何预设布局能完全匹配时，找到尺寸最接近的布局
 * 使用曼哈顿距离 (|cols - cols| + |rows - rows|) 衡量接近程度
 *
 * @param storage - 预设布局存储
 * @param size - 当前网格尺寸
 * @returns 最接近的布局项，或 null（存储为空时）
 */
export function findClosestStoredLayout<T extends DndGridItemType<string>>(
  storage: Record<string, T[]>,
  size: GridSize,
): T[] | null {
  let best: { dist: number; items: T[] } | null = null;

  for (const [key, items] of Object.entries(storage)) {
    const match = key.match(/^(\d+)x(\d+)$/);
    if (!match) continue;

    const cols = parseInt(match[1], 10);
    const rows = parseInt(match[2], 10);
    const dist = Math.abs(cols - size.cols) + Math.abs(rows - size.rows);

    if (!best || dist < best.dist) {
      best = { dist, items };
    }
  }

  return best?.items ?? null;
}

/**
 * 检查候选位置是否与已放置列表中的项重叠（排除自身）
 */
function hasOverlapWith<T extends DndGridItemType<string>>(
  placed: T[],
  candidate: T,
): boolean {
  return placed.some((p) => p.id !== candidate.id && isOverlap(p, candidate));
}

/**
 * 自上而下、自左而右扫描，找到第一个可容纳 (w × h) 的空闲位置
 *
 * @returns 放置后的项，或 null（无空闲位置）
 */
function tryPlace<T extends DndGridItemType<string>>(
  item: T,
  w: number,
  h: number,
  placed: T[],
  cols: number,
  rows: number,
): T | null {
  for (let y = 0; y + h <= rows; y++) {
    for (let x = 0; x + w <= cols; x++) {
      const candidate = { ...item, x, y, w, h } as T;

      if (!hasOverlapWith(placed, candidate)) {
        return candidate;
      }
    }
  }
  return null;
}

/**
 * 自适应布局算法
 *
 * 将给定的 items 适应到新的网格尺寸中。处理顺序按阅读顺序（top→bottom, left→right），
 * 先处理的项有更高的优先级。
 *
 * 适应策略（逐项应用）：
 * 1. clamp：将 (x,y) 限制在边界内，保持原始 (w,h)，若无重叠则保留
 * 2. 扫描：在当前 (w,h) 下找空闲位置
 * 3. 缩小：向 (minW, minH) 方向逐步缩小 (w,h) 并重试
 * 4. 丢弃：如果最小尺寸也无法放置，丢弃该项
 *
 * @param items - 原始布局项列表
 * @param size - 目标网格尺寸
 * @param constraints - 各项的尺寸约束
 * @returns 自适应后的布局项列表
 */
export function adaptLayout<T extends DndGridItemType<string>>(
  items: T[],
  size: GridSize,
  constraints: Record<string, GridItemConstraints>,
): T[] {
  const { cols, rows } = size;
  const result: T[] = [];

  // 按阅读顺序排序
  const sorted = [...items].sort((a, b) =>
    a.y !== b.y ? a.y - b.y : a.x - b.x,
  );

  for (const item of sorted) {
    const c = constraints[item.id] ?? {};
    const minW = c.minW ?? 1;
    const minH = c.minH ?? 1;

    // 即使最小尺寸也放不下 → 丢弃
    if (minW > cols || minH > rows) continue;

    // 将尺寸限制在 [minW..cols] 和 [minH..rows] 范围内
    const w = Math.max(minW, Math.min(item.w, cols));
    const h = Math.max(minH, Math.min(item.h, rows));
    // 将位置限制在边界内
    const x = Math.max(0, Math.min(item.x, cols - w));
    const y = Math.max(0, Math.min(item.y, rows - h));

    const clamped = { ...item, x, y, w, h } as T;

    // 步骤 1：尝试 clamp 后的位置（无重叠）
    if (!hasOverlapWith(result, clamped)) {
      result.push(clamped);
      continue;
    }

    // 步骤 2：在当前 (w, h) 下扫描空闲位置
    const placed = tryPlace(item, w, h, result, cols, rows);
    if (placed) {
      result.push(placed);
      continue;
    }

    // 步骤 3：逐渐缩小 (w, h) 并向 (minW, minH) 靠近并重试
    const findShrinkPlacement = (): T | null => {
      for (let tw = w; tw >= minW; tw--) {
        for (let th = h; th >= minH; th--) {
          if (tw === w && th === h) continue; // 已在步骤 2 尝试过

          const p = tryPlace(item, tw, th, result, cols, rows);

          if (p) {
            return p;
          }
        }
      }

      return null;
    };

    const found = findShrinkPlacement();
    if (found) result.push(found);
    // else: 丢弃该项
  }

  return result;
}
