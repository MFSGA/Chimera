import type { ProfileQueryResultItem } from '@chimera/interface';
import { cn } from '@chimera/ui';
import { useSortable } from '@dnd-kit/react/sortable';
import { motion } from 'motion/react';
import ProfileCard from './profile-card';

export default function SortableProfileItem({
  item,
  index,
  disabled,
}: {
  item: ProfileQueryResultItem;
  index: number;
  disabled: boolean;
}) {
  const { ref, isDragging } = useSortable({
    id: item.uid,
    index,
    disabled,
  });

  return (
    <motion.div
      ref={(element: HTMLDivElement | null) => ref(element)}
      className={cn(
        'min-w-0 cursor-grab transition-opacity active:cursor-grabbing',
        isDragging && 'opacity-40',
      )}
      layoutId={`profile-${item.uid}`}
      layout="position"
      initial={false}
    >
      <ProfileCard profile={item} />
    </motion.div>
  );
}
