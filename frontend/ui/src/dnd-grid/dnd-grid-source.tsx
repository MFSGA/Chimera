import { cn } from '@chimera/utils';
import { useDraggable } from '@dnd-kit/core';
import type { ComponentProps } from 'react';
import type { DndGridSourceData } from './root-context';

export type DndGridSourceProps = Omit<ComponentProps<'div'>, 'ref'> & {
  /** The id the destination grid receives through `onExternalDrop`. */
  itemId: string;
  /** Keeps the drag id distinct from item ids already on a grid. */
  dragIdPrefix: string;
  /** Size in grid cells of the dragged preview. */
  w: number;
  h: number;
  /** Prevent new drags while the destination layout/config is unreadable. */
  disabled?: boolean;
  onSourceDragStart?: () => void;
};

/**
 * Drags a new item from outside any grid, such as a library list, onto a
 * `DndGrid` registered in the same `DndGridRoot`.
 */
export function DndGridSource({
  itemId,
  dragIdPrefix,
  w,
  h,
  disabled = false,
  onSourceDragStart,
  className,
  ...props
}: DndGridSourceProps) {
  const data: DndGridSourceData = {
    kind: 'dnd-grid-source',
    itemId,
    dragIdPrefix,
    w,
    h,
    onDragStart: onSourceDragStart,
  };
  const { listeners, setNodeRef } = useDraggable({
    id: `${dragIdPrefix}${itemId}`,
    data,
    disabled,
  });

  return (
    <div
      ref={setNodeRef}
      data-slot="dnd-grid-source"
      className={cn(
        disabled ? 'cursor-not-allowed opacity-60' : 'cursor-grab',
        className,
      )}
      aria-disabled={disabled || undefined}
      {...props}
      {...listeners}
    />
  );
}
