/**
 * DnD Grid 项组件
 *
 * 迁移自 ref: `src/components/ui/dnd-grid/dnd-grid-item.tsx`
 *
 * 职责：
 * - 渲染单个网格项，使用 @dnd-kit 的 useDraggable 实现拖拽
 * - 使用 framer-motion 的 useSpring 实现放置后弹性动画
 * - 提供调整大小的拖拽手柄（ResizeKnob）
 * - 支持覆盖层模式（DragOverlay 内渲染时跳过定位逻辑）
 *
 * 两种渲染模式：
 * 1. isOverlay: 仅在 DragOverlay 内使用，简单包裹子元素以填充 overlay div
 * 2. 普通模式：使用 DndGridItemDraggable 包含完整拖拽和定位逻辑
 */

import {
  AnimatePresence,
  motion,
  useSpring,
  type Transition,
} from 'framer-motion';
import { useLayoutEffect, useRef, type PropsWithChildren } from 'react';
import { useDraggable } from '@dnd-kit/core';
import { cn } from '@chimera/ui';
import { useDndGridContext } from './context';
import type { GridItemConstraints } from './types';

/** 位置弹性动画配置 */
const SPRING_OPTIONS = {
  stiffness: 350,
  damping: 35,
} as Transition;

/** 调整大小时过渡动画配置 */
const RESIZE_SPRING = {
  type: 'spring',
  stiffness: 400,
  damping: 35,
} as Transition;

/** 普通布局变化的瞬间过渡（无动画） */
const INSTANT = {
  duration: 0,
} as Transition;

export type DndGridItemProps<T extends string> = PropsWithChildren<{
  id: T;
  className?: string;
}> &
  GridItemConstraints;

/**
 * 调整大小手柄组件
 *
 * 位于项右下角，使用 Pointer Events 实现拖拽
 * 通过 onPointerDown/Move/Up 跟踪拖拽坐标变化
 */
function ResizeKnob({
  onStart,
  onMove,
  onEnd,
}: {
  onStart: (x: number, y: number) => void;
  onMove: (x: number, y: number) => void;
  onEnd: () => void;
}) {
  return (
    <motion.div
      className={cn(
        'absolute -right-0.75 -bottom-0.75 z-20 flex size-7 items-center justify-center',
        'text-on-surface',
        'touch-none select-none',
      )}
      data-slot="resize-handle"
      onPointerDown={(e) => {
        e.preventDefault();
        e.stopPropagation();
        e.currentTarget.setPointerCapture(e.pointerId);
        onStart(e.clientX, e.clientY);
      }}
      onPointerMove={(e) => {
        if (!e.currentTarget.hasPointerCapture(e.pointerId)) {
          return;
        }

        onMove(e.clientX, e.clientY);
      }}
      onPointerUp={(e) => {
        if (!e.currentTarget.hasPointerCapture(e.pointerId)) {
          return;
        }

        e.currentTarget.releasePointerCapture(e.pointerId);
        onEnd();
      }}
      onPointerCancel={(e) => {
        if (!e.currentTarget.hasPointerCapture(e.pointerId)) {
          return;
        }

        e.currentTarget.releasePointerCapture(e.pointerId);
        onEnd();
      }}
      initial={{
        scale: 0.85,
        opacity: 0,
      }}
      animate={{
        scale: 1,
        opacity: 1,
      }}
      exit={{
        scale: 0.85,
        opacity: 0,
      }}
      transition={{
        type: 'tween',
        duration: 0.1,
        ease: 'easeOut',
      }}
    >
      {/* 右下角拖拽手柄图标 */}
      <svg
        className="size-full cursor-se-resize"
        viewBox="11 11 7 7"
        fill="none"
        data-slot="resize-handle-icon"
      >
        <path
          d="M12 17.25H13A4.25 4.25 0 0 0 17.25 13V12"
          stroke="currentColor"
          strokeWidth="1.5"
          strokeLinecap="round"
        />
      </svg>
    </motion.div>
  );
}

/**
 * 可拖拽的网格项
 *
 * 包含完整的定位、拖拽和调整大小逻辑：
 * - 使用 useDraggable 注册拖拽
 * - 使用 useSpring 实现放置后回弹动画
 * - 在编辑模式下显示调整大小手柄
 * - 拖拽时自身变为透明，由 DragOverlay 替代显示
 */
function DndGridItemDraggable<T extends string>({
  id,
  className,
  children,
  minW,
  minH,
  maxW,
  maxH,
}: DndGridItemProps<T>) {
  const {
    displayItems,
    getItemRect,
    dropInfoMap,
    activeItemId,
    resizingItemId,
    disabled,
    sourceOnly,
    dragIdPrefix,
    constraintsMapRef,
    onResizeStart,
    onResizeMove,
    onResizeEnd,
  } = useDndGridContext();

  // 在每次渲染时同步约束（确保调整大小前约束始终最新）
  constraintsMapRef.current[id] = { minW, minH, maxW, maxH };

  const item = displayItems.find((i) => i.id === id);

  // 拖拽禁用条件：disabled 或没有任何其他项正在被调整大小
  const { attributes, listeners, setNodeRef } = useDraggable({
    id: dragIdPrefix ? `${dragIdPrefix}${id}` : id,
    disabled: disabled || !item || resizingItemId !== null,
    data: item,
  });

  // 放置后弹性动画
  const springX = useSpring(0, SPRING_OPTIONS);
  const springY = useSpring(0, SPRING_OPTIONS);

  const dropInfo = dropInfoMap[id];
  const prevDropInfoRef = useRef<typeof dropInfo>(undefined);

  // 当有新的放置信息时，跳转到放置位置并弹回
  useLayoutEffect(() => {
    if (!dropInfo || dropInfo === prevDropInfoRef.current || !item) {
      return;
    }

    prevDropInfoRef.current = dropInfo;
    const rect = getItemRect(item);
    springX.jump(dropInfo.left - rect.left);
    springY.jump(dropInfo.top - rect.top);
    springX.set(0);
    springY.set(0);
  }, [dropInfo, item, getItemRect, springX, springY]);

  if (!item) {
    return null;
  }

  const rect = getItemRect(item);
  const isActiveItem = activeItemId === id;
  const isResizing = resizingItemId === id;

  return (
    <motion.div
      ref={setNodeRef}
      initial={false}
      animate={{
        left: rect.left,
        top: rect.top,
        width: rect.width,
        height: rect.height,
      }}
      transition={
        isResizing
          ? {
              left: RESIZE_SPRING,
              top: RESIZE_SPRING,
              width: RESIZE_SPRING,
              height: RESIZE_SPRING,
            }
          : {
              left: INSTANT,
              top: INSTANT,
              width: INSTANT,
              height: INSTANT,
            }
      }
      className={cn('group', className)}
      style={{
        position: 'absolute',
        touchAction: 'none',
        opacity: isActiveItem ? 0 : 1,
        x: springX,
        y: springY,
      }}
      {...attributes}
      {...listeners}
    >
      {children}

      {/* 编辑模式下显示调整大小手柄 */}
      <AnimatePresence>
        {!disabled && !sourceOnly && (
          <ResizeKnob
            onStart={(x, y) => onResizeStart(id, 'bottom-right', x, y)}
            onMove={onResizeMove}
            onEnd={onResizeEnd}
          />
        )}
      </AnimatePresence>
    </motion.div>
  );
}

/**
 * DndGridItem 组件 - 入口
 *
 * 根据是否为覆盖层模式分派到不同的渲染路径：
 * - isOverlay: 直接包裹 children，不执行定位（由 DragOverlay 的样式控制）
 * - 普通模式: 使用 DndGridItemDraggable 包含完整功能
 */
export function DndGridItem<T extends string>({
  id,
  className,
  children,
  minW,
  minH,
  maxW,
  maxH,
}: DndGridItemProps<T>) {
  const { isOverlay } = useDndGridContext();

  if (isOverlay) {
    // DragOverlay 内部：跳过定位和拖拽逻辑，只填充 overlay 容器
    return <div className={cn('size-full', className)}>{children}</div>;
  }

  return (
    <DndGridItemDraggable
      id={id}
      className={className}
      minW={minW}
      minH={minH}
      maxW={maxW}
      maxH={maxH}
    >
      {children}
    </DndGridItemDraggable>
  );
}
