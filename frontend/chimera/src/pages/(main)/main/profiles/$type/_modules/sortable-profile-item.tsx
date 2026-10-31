import type { ProfileQueryResultItem } from '@chimera/interface';
import { cn } from '@chimera/ui';
import { pointerIntersection } from '@dnd-kit/collision';
import { useSortable } from '@dnd-kit/react/sortable';
import { motion } from 'motion/react';
import { WHOLE_ITEM_DRAG_SENSORS } from '@/utils/dnd-sensors';
import ProfileCard from '../../_modules/profile-card';
import { PROFILE_DRAG_TYPE } from '../../_modules/resolve-profile-drop';

export default function SortableProfileItem({
  item,
  profiles,
  isGlobal,
  index,
  disabled,
  group,
}: {
  item: ProfileQueryResultItem;
  profiles: ProfileQueryResultItem[];
  isGlobal: boolean;
  index: number;
  disabled: boolean;
  /** Config cards participate in the parent Profile-section DnD scope. */
  group?: string;
}) {
  const { ref, isDragging } = useSortable({
    id: item.uid,
    index,
    group,
    disabled,
    ...(group
      ? {
          type: PROFILE_DRAG_TYPE,
          accept: [PROFILE_DRAG_TYPE],
          collisionDetector: pointerIntersection,
          sensors: WHOLE_ITEM_DRAG_SENSORS,
        }
      : {}),
  });

  return (
    <motion.div
      ref={(element: HTMLDivElement | null) => ref(element)}
      className={cn(
        'min-w-0 cursor-grab transition-opacity active:cursor-grabbing',
        isDragging && 'opacity-40',
      )}
      data-slot="profile-card-sortable"
      data-profile-uid={item.uid}
      data-dragging={String(isDragging)}
      data-drag-disabled={String(disabled)}
      layoutId={`profile-${item.uid}`}
      layout="position"
      initial={false}
    >
      <ProfileCard profile={item} profiles={profiles} isGlobal={isGlobal} />
    </motion.div>
  );
}
