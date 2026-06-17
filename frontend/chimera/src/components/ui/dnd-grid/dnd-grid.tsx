/**
 * DnD Grid 组件
 *
 * 迁移自 ref: `src/components/ui/dnd-grid/dnd-grid.tsx`
 *
 * 职责：
 * - 提供可拖拽、可调整大小的网格布局
 * - 使用 @dnd-kit 实现拖拽交互
 * - 支持两种模式：
 *   1. 独立模式：自身包含 DndContext，适合单网格页面
 *   2. 受管模式：注册到 DndGridRoot，适合多网格共存页面（如 Dashboard + WidgetSheet）
 * - 通过 resize 手柄支持项的大小调整
 * - 渲染占位符、拖拽覆盖层和动画过渡
 *
 * 核心流程：
 * 1. useGridLayout 监听容器尺寸并计算网格布局
 * 2. items 按网格坐标渲染为绝对定位的子元素
 * 3. 拖拽时显示幽灵占位符，目标位置用虚线框预览
 * 4. 放置后通过 onLayoutChange 通知外部更新
 */

import { AnimatePresence, motion } from 'framer-motion';
import {
  Fragment,
  useCallback,
  useEffect,
  useLayoutEffect,
  useRef,
  useState,
} from 'react';
import {
  DndContext,
  DragOverlay,
  PointerSensor,
  TouchSensor,
  useSensor,
  useSensors,
  type DragEndEvent,
  type DragMoveEvent,
  type DragStartEvent,
} from '@dnd-kit/core';
import { cn } from '@chimera/ui';
import { DndGridProvider } from './context';
import { useDndGridRoot, type GridRegistration } from './root-context';
import type {
  DndGridItemType,
  GridItemConstraints,
  GridSize,
  ResizeHandle,
} from './types';
import { useGridLayout } from './use-grid-layout';
import { calculateResize, hasOverlap } from './utils';

export interface DndGridProps<T extends string = string> {
  items: DndGridItemType<T>[];
  onLayoutChange?: (items: DndGridItemType<T>[]) => void;
  minCellSize?: number;
  gap?: number;
  size?: GridSize;
  onSizeChange?: (
    size: GridSize,
    constraintsMap: Record<string, GridItemConstraints>,
  ) => void;
  children: (item: DndGridItemType<T>) => React.ReactNode;
  className?: string;
  disabled?: boolean;
  sourceOnly?: boolean;
  dragIdPrefix?: string;
  gridId?: string;
  onSourceDrop?: (itemId: string) => void;
  onSourceDragStart?: () => void;
}

export function DndGrid<T extends string = string>({
  items,
  onLayoutChange,
  minCellSize = 96,
  gap = 8,
  size,
  onSizeChange,
  children,
  className,
  disabled = true,
  sourceOnly = false,
  dragIdPrefix = '',
  gridId,
  onSourceDrop,
  onSourceDragStart,
}: DndGridProps<T>) {
  const constraintsMapRef = useRef<Record<string, GridItemConstraints>>({});

  const { containerRef, layout, computedSize, getItemRect, snapToGrid } =
    useGridLayout(minCellSize, gap, size);

  const onSizeChangeRef = useRef(onSizeChange);
  onSizeChangeRef.current = onSizeChange;

  // 当网格尺寸变化时通知外部（用于 Dashboard 布局自适应）
  useEffect(() => {
    if (computedSize) {
      onSizeChangeRef.current?.(computedSize, constraintsMapRef.current);
    }
  }, [computedSize]);

  const [activeItem, setActiveItem] = useState<DndGridItemType<T> | null>(null);
  const [previewItem, setPreviewItem] = useState<DndGridItemType<T> | null>(
    null,
  );

  const [displayItems, setDisplayItems] = useState<DndGridItemType<T>[]>(items);
  const [dropInfoMap, setDropInfoMap] = useState<
    Record<string, { left: number; top: number }>
  >({});

  const isDragging = useRef(false);
  const lastValidSnapRef = useRef<DndGridItemType<T> | null>(null);

  // 调整大小状态
  const resizeStateRef = useRef<{
    id: string;
    handle: ResizeHandle;
    startItem: DndGridItemType<T>;
    startX: number;
    startY: number;
  } | null>(null);
  const resizePreviewRef = useRef<DndGridItemType<T> | null>(null);

  const [resizingItemId, setResizingItemId] = useState<string | null>(null);
  const [resizePreview, setResizePreview] = useState<DndGridItemType<T> | null>(
    null,
  );

  // 当外部 items 变化时同步到内部状态（仅在非拖拽状态时）
  useEffect(() => {
    if (!isDragging.current && !resizeStateRef.current) {
      setDisplayItems(items);
    }
  }, [items]);

  // 调整大小时的预览替换
  const effectiveDisplayItems = resizePreview
    ? displayItems.map((item) =>
        item.id === resizePreview.id ? resizePreview : item,
      )
    : displayItems;

  const sensors = useSensors(
    useSensor(PointerSensor, { activationConstraint: { distance: 6 } }),
    useSensor(TouchSensor, {
      activationConstraint: { delay: 200, tolerance: 6 },
    }),
  );

  /** 拖拽开始：记录拖拽项并清空重叠状态 */
  const handleDragStart = useCallback(
    ({ active }: DragStartEvent) => {
      const item = items.find((i) => i.id === active.id);

      if (!item) {
        return;
      }

      isDragging.current = true;
      lastValidSnapRef.current = item;
      setActiveItem(item);
      setPreviewItem(item);
      setDisplayItems(items);
    },
    [items],
  );

  /** 拖拽移动：计算网格对齐位置，检测重叠 */
  const handleDragMove = useCallback(
    ({ delta }: DragMoveEvent) => {
      if (!activeItem) {
        return;
      }

      const snapped = snapToGrid(activeItem, delta.x, delta.y);

      // 仅在目标位置空闲时更新占位符
      if (!hasOverlap(items, activeItem.id, snapped)) {
        lastValidSnapRef.current = snapped;
        setPreviewItem(snapped);
      }
      // 重叠时占位符保持在 lastValidSnapRef，无需更新状态
    },
    [activeItem, items, snapToGrid],
  );

  /** 拖拽结束：确定最终位置，通知外部 */
  const handleDragEnd = useCallback(
    ({ active, delta }: DragEndEvent) => {
      if (!activeItem) {
        return;
      }

      const originalRect = getItemRect(activeItem);
      const dropLeft = originalRect.left + delta.x;
      const dropTop = originalRect.top + delta.y;

      const snapped = snapToGrid(activeItem, delta.x, delta.y);
      // 如果对齐位置空闲则使用，否则回退到最后一个有效位置
      const finalItem = !hasOverlap(items, activeItem.id, snapped)
        ? snapped
        : (lastValidSnapRef.current ?? activeItem);

      const newItems = items.map((i) => (i.id === active.id ? finalItem : i));
      const id = String(active.id);

      isDragging.current = false;
      lastValidSnapRef.current = null;

      setActiveItem(null);
      setPreviewItem(null);
      setDisplayItems(newItems);
      setDropInfoMap((prev) => ({
        ...prev,
        [id]: { left: dropLeft, top: dropTop },
      }));
      onLayoutChange?.(newItems);
    },
    [activeItem, items, snapToGrid, getItemRect, onLayoutChange],
  );

  /** 拖拽取消：恢复原始状态 */
  const handleDragCancel = useCallback(() => {
    isDragging.current = false;
    lastValidSnapRef.current = null;
    setActiveItem(null);
    setPreviewItem(null);
    setDisplayItems(items);
  }, [items]);

  /** 调整大小开始 */
  const onResizeStart = useCallback(
    (id: string, handle: ResizeHandle, startX: number, startY: number) => {
      const item = items.find((i) => i.id === id);

      if (!item || disabled) {
        return;
      }

      resizeStateRef.current = {
        id,
        handle,
        startItem: item,
        startX,
        startY,
      };

      setResizingItemId(id);
    },
    [items, disabled],
  );

  /** 调整大小移动 */
  const onResizeMove = useCallback(
    (currentX: number, currentY: number) => {
      const state = resizeStateRef.current;

      if (!state) {
        return;
      }

      const { cellW, cellH, cols, rows } = layout;
      const deltaX = currentX - state.startX;
      const deltaY = currentY - state.startY;
      const candidate = calculateResize(
        state.startItem,
        state.handle,
        deltaX,
        deltaY,
        cellW,
        cellH,
        gap,
        cols,
        rows,
        constraintsMapRef.current[state.id],
      );

      if (!hasOverlap(items, state.id, candidate)) {
        resizePreviewRef.current = candidate;
        setResizePreview(candidate);
      }
    },
    [items, layout, gap],
  );

  /** 调整大小结束 */
  const onResizeEnd = useCallback(() => {
    const preview = resizePreviewRef.current;
    resizeStateRef.current = null;
    resizePreviewRef.current = null;
    setResizingItemId(null);
    setResizePreview(null);

    if (preview) {
      const newItems = items.map((i) => (i.id === preview.id ? preview : i));
      setDisplayItems(newItems);
      onLayoutChange?.(newItems);
    }
  }, [items, onLayoutChange]);

  const rootCtx = useDndGridRoot();

  // 稳定引用对象，每次渲染时更新属性（用于 DndGridRoot 注册）
  // 这样 root 始终读取最新的回调闭包，而引用不变避免重复 effect
  const registrationRef = useRef<GridRegistration>({
    itemIds: [],
    dragIdPrefix: '',
    sourceOnly: false,
    handleDragStart: () => {},
    handleDragMove: () => {},
    handleDragEnd: () => {},
    handleDragCancel: () => {},
    getCellSize: () => ({ cellW: 0, cellH: 0, gap: 0 }),
  });

  Object.assign(registrationRef.current, {
    itemIds: items.map((i) => i.id),
    dragIdPrefix,
    sourceOnly,
    handleDragStart,
    handleDragMove,
    handleDragEnd,
    handleDragCancel,
    getCellSize: () => ({ cellW: layout.cellW, cellH: layout.cellH, gap }),
    onSourceDrop,
    onSourceDragStart,
  });

  // 注册到 DndGridRoot（仅在受管模式下）
  useLayoutEffect(() => {
    if (!rootCtx || !gridId) {
      return;
    }

    rootCtx.registerGrid(gridId, registrationRef.current);

    return () => {
      rootCtx.unregisterGrid(gridId);
    };
  }, [rootCtx, gridId]);

  const isManaged = Boolean(rootCtx && gridId);

  const overlayRect = activeItem ? getItemRect(activeItem) : null;
  const placeholderRect = previewItem ? getItemRect(previewItem) : null;

  // 网格内容：提供 DndGridProvider + 容器 + 占位符 + 项渲染 + 覆盖层
  const gridContent = (
    <DndGridProvider
      value={{
        displayItems: effectiveDisplayItems,
        getItemRect,
        dropInfoMap,
        activeItemId: activeItem?.id ?? null,
        resizingItemId,
        disabled,
        sourceOnly,
        dragIdPrefix,
        isOverlay: false,
        constraintsMapRef,
        onResizeStart,
        onResizeMove,
        onResizeEnd,
      }}
    >
      <div
        ref={containerRef}
        className={cn('relative', className)}
        data-slot="dnd-grid-container"
      >
        {/* 拖拽占位符虚线框 */}
        <AnimatePresence>
          {placeholderRect && activeItem && (
            <motion.div
              key="dnd-grid-placeholder"
              data-slot="dnd-grid-placeholder"
              layout
              className={cn(
                'border-primary/40 bg-primary/5 border-2 border-dashed',
                'pointer-events-none absolute rounded-2xl',
              )}
              style={{
                left: placeholderRect.left,
                top: placeholderRect.top,
                width: placeholderRect.width,
                height: placeholderRect.height,
              }}
              initial={{ opacity: 0 }}
              animate={{ opacity: 1 }}
              exit={{ opacity: 0 }}
              transition={{ type: 'spring', stiffness: 300, damping: 28 }}
            />
          )}
        </AnimatePresence>

        {/* 渲染所有网格项 */}
        {effectiveDisplayItems.map((item) => (
          <Fragment key={item.id}>{children(item)}</Fragment>
        ))}
      </div>

      {/* 独立模式（非受管）下的 DragOverlay */}
      {!isManaged && (
        <DragOverlay dropAnimation={null}>
          <AnimatePresence>
            {activeItem && overlayRect && (
              <motion.div
                key="dnd-grid-overlay"
                data-slot="dnd-grid-overlay"
                className="cursor-grabbing"
                style={{ width: overlayRect.width, height: overlayRect.height }}
                initial={{ opacity: 0.85 }}
                animate={{ opacity: 0.95 }}
                exit={{ opacity: 0 }}
                transition={{ type: 'tween', duration: 0.1, ease: 'easeOut' }}
              >
                <DndGridProvider
                  value={{
                    displayItems: effectiveDisplayItems,
                    getItemRect,
                    dropInfoMap,
                    activeItemId: activeItem?.id ?? null,
                    resizingItemId,
                    disabled,
                    sourceOnly,
                    dragIdPrefix,
                    isOverlay: true,
                    constraintsMapRef,
                    onResizeStart,
                    onResizeMove,
                    onResizeEnd,
                  }}
                >
                  {children(activeItem)}
                </DndGridProvider>
              </motion.div>
            )}
          </AnimatePresence>
        </DragOverlay>
      )}
    </DndGridProvider>
  );

  // 受管模式：gridContent 不包含 DndContext（由 DndGridRoot 提供）
  if (isManaged) {
    return gridContent;
  }

  // 独立模式：包裹 DndContext
  return (
    <DndContext
      sensors={sensors}
      onDragStart={handleDragStart}
      onDragMove={handleDragMove}
      onDragEnd={handleDragEnd}
      onDragCancel={handleDragCancel}
    >
      {gridContent}
    </DndContext>
  );
}
