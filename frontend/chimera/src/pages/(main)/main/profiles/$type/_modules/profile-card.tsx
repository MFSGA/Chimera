import type { ProfileQueryResultItem } from '@chimera/interface';
import { cn } from '@chimera/ui';
import { LinearProgress, Menu, MenuItem } from '@mui/material';
import { Link } from '@tanstack/react-router';
import DeleteForeverOutlineRounded from '~icons/material-symbols/delete-forever-outline-rounded';
import DragClickRounded from '~icons/material-symbols/drag-click-rounded';
import { AnimatePresence, motion } from 'motion/react';
import { useState, type ComponentProps } from 'react';
import { Button } from '@/components/ui/button';
import {
  Card,
  CardContent,
  CardFooter,
  CardHeader,
} from '@/components/ui/card';
import TextMarquee from '@/components/ui/text-marquee';
import * as m from '@/paraglide/messages';
import { Route as IndexRoute } from '../index';
import { useActiveProfile, useDeleteProfile } from './profile-actions';

const Chip = ({ children, className, ...props }: ComponentProps<'span'>) => (
  <span
    className={cn(
      'bg-primary-container rounded-full px-3 py-1 text-xs font-bold whitespace-nowrap',
      className,
    )}
    {...props}
  >
    {children}
  </span>
);

export default function ProfileCard({
  profile,
}: {
  profile: ProfileQueryResultItem;
}) {
  const { type } = IndexRoute.useParams();
  const activeProfile = useActiveProfile(profile);
  const deleteProfile = useDeleteProfile(profile);
  const isPending = activeProfile.isPending || deleteProfile.isPending;
  const [menuPosition, setMenuPosition] = useState<{
    top: number;
    left: number;
  } | null>(null);

  const closeMenu = () => setMenuPosition(null);

  return (
    <>
      <Card
        className="relative min-h-40 overflow-hidden"
        data-slot="profile-card"
        onContextMenu={(event) => {
          event.preventDefault();
          setMenuPosition({ top: event.clientY, left: event.clientX });
        }}
      >
        <AnimatePresence initial={false}>
          {isPending && (
            <motion.div
              className={cn(
                'bg-primary/10 absolute inset-0 z-50 backdrop-blur-3xl',
                'flex flex-col items-center justify-center gap-2',
              )}
              data-slot="profile-card-mask"
              initial={{ opacity: 0 }}
              animate={{ opacity: 1 }}
              exit={{ opacity: 0 }}
            >
              <LinearProgress className="w-2/3 max-w-60" />
              <p className="text-on-surface-variant text-xs">
                {m.profile_pending_mask_message()}
              </p>
            </motion.div>
          )}
        </AnimatePresence>

        {activeProfile.isActive && (
          <div
            className="from-primary/25 via-primary-container/20 absolute inset-0 bg-gradient-to-br to-transparent"
            data-slot="profile-card-active-background"
          />
        )}

        <CardHeader className="relative flex items-center justify-between gap-2">
          <TextMarquee className="min-w-0 flex-1">{profile.name}</TextMarquee>
          {activeProfile.isActive && (
            <Chip className="shrink-0">{m.profile_is_active_label()}</Chip>
          )}
        </CardHeader>

        <CardContent className="relative">
          <Chip>
            {profile.type === 'remote'
              ? m.profile_remote_label()
              : m.profile_local_label()}
          </Chip>
        </CardContent>

        <CardFooter className="relative">
          <Button asChild>
            <Link
              to="/main/profiles/$type/detail/$uid"
              params={{ type, uid: profile.uid }}
            >
              {m.profile_view_details_title()}
            </Link>
          </Button>
        </CardFooter>
      </Card>

      <Menu
        open={Boolean(menuPosition)}
        onClose={closeMenu}
        anchorReference="anchorPosition"
        anchorPosition={menuPosition ?? undefined}
      >
        <MenuItem
          disabled={isPending}
          onClick={() => {
            closeMenu();
            void activeProfile.handleClick();
          }}
        >
          <DragClickRounded className="mr-2 size-4" />
          {m.profile_active_title()}
        </MenuItem>
        <MenuItem
          disabled={isPending}
          onClick={() => {
            closeMenu();
            void deleteProfile.handleClick();
          }}
        >
          <DeleteForeverOutlineRounded className="mr-2 size-4" />
          {m.profile_delete_title()}
        </MenuItem>
      </Menu>
    </>
  );
}
