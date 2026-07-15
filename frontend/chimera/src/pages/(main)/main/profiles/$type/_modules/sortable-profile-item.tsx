import { cn } from '@chimera/ui';
import { useSortable } from '@dnd-kit/react/sortable';
import { Grid } from '@mui/material';
import { motion } from 'motion/react';
import type { ClashProfile } from '@/components/profiles/utils';
import ProfileCard from './profile-card';

export default function SortableProfileItem({
  item,
  index,
  disabled,
}: {
  item: ClashProfile;
  index: number;
  disabled: boolean;
}) {
  const { ref, isDragging } = useSortable({
    id: item.uid,
    index,
    disabled,
  });

  return (
    <Grid
      ref={(element: HTMLDivElement | null) => ref(element)}
      size={{ xs: 12, sm: 12, md: 6, lg: 4, xl: 3 }}
    >
      <motion.div
        className={cn(
          'cursor-grab transition-opacity active:cursor-grabbing',
          isDragging && 'opacity-40',
        )}
        layoutId={`profile-${item.uid}`}
        layout="position"
        initial={false}
      >
        <ProfileCard profile={item} />
      </motion.div>
    </Grid>
  );
}
