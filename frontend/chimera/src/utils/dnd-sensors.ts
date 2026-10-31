import { PointerActivationConstraints } from '@dnd-kit/dom';
import {
  DragDropProvider,
  KeyboardSensor,
  PointerSensor,
} from '@dnd-kit/react';
import type { useSortable } from '@dnd-kit/react/sortable';

type SortableSensors = NonNullable<
  Parameters<typeof useSortable>[0]['sensors']
>;

/**
 * Sensors for lists whose whole item is the drag source. Without a drag handle
 * the default mouse activation requires a press-and-hold; touch keeps its delay
 * to stay scrollable.
 */
export const WHOLE_ITEM_DRAG_SENSORS: SortableSensors = [
  PointerSensor.configure({
    activationConstraints: (event, source) => {
      if (event.pointerType === 'mouse') return undefined;

      const defaults = PointerSensor.defaults.activationConstraints;

      return typeof defaults === 'function'
        ? defaults(event, source)
        : defaults;
    },
  }),
  KeyboardSensor,
];

/** Match ref: sidebar folder links activate drag after a pointer moved 8px. */
export const FOLDER_ITEM_DRAG_SENSORS: SortableSensors = [
  PointerSensor.configure({
    activationConstraints: (event, source) => {
      if (event.pointerType !== 'touch') {
        return [new PointerActivationConstraints.Distance({ value: 8 })];
      }
      const defaults = PointerSensor.defaults.activationConstraints;
      return typeof defaults === 'function'
        ? defaults(event, source)
        : defaults;
    },
  }),
  KeyboardSensor,
];
