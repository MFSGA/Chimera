import {
  getRemoteSource,
  isConfigItem,
  isTransformItem,
  type ProfileQueryResultItem,
} from '@chimera/interface';
import { cn } from '@chimera/ui';
import { Button } from '@chimera/ui/button';
import { Card, CardContent, CardFooter, CardHeader } from '@chimera/ui/card';
import { ContextMenuItem } from '@chimera/ui/context-menu';
import TextMarquee from '@chimera/ui/text-marquee';
import { Link } from '@tanstack/react-router';
import DeleteForeverOutlineRounded from '~icons/material-symbols/delete-forever-outline-rounded';
import DragClickRounded from '~icons/material-symbols/drag-click-rounded';
import EditSquareOutlineRounded from '~icons/material-symbols/edit-square-outline-rounded';
import { AnimatePresence, motion } from 'motion/react';
import type { ComponentProps } from 'react';
import {
  RegisterContextMenu,
  RegisterContextMenuContent,
  RegisterContextMenuTrigger,
} from '@/components/providers/context-menu-provider';
import * as m from '@/paraglide/messages';
import { Route as IndexRoute } from '../$type/index';
import ProfileCardAction from './profile-card-action';
import { useActiveProfile } from './use-active-profile';
import { useDeleteProfile } from './use-delete-profile';
import { hasMaterializedFile, isProxyProfile } from './utils';
import ViewContent from './view-content';

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
  const isProxy = isProxyProfile(profile);

  const typeLabel = (() => {
    if (isConfigItem(profile)) {
      if (profile.config.type === 'composition') return 'Composition';
      return getRemoteSource(profile)
        ? m.profile_remote_label()
        : m.profile_local_label();
    }
    if (isTransformItem(profile) && profile.transform.type === 'overlay') {
      return m.profile_merge_label();
    }
    if (isTransformItem(profile) && profile.transform.type === 'script') {
      return profile.transform.runtime === 'lua'
        ? m.profile_lua_label()
        : m.profile_javascript_label();
    }
    return '';
  })();

  return (
    <RegisterContextMenu>
      <RegisterContextMenuTrigger asChild>
        <Card
          className="relative flex min-h-40 flex-col justify-between overflow-hidden"
          data-slot="profile-card"
          data-profile-uid={profile.uid}
          data-profile-active={String(activeProfile.isActive)}
          asChild
        >
          <div>
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
                  <div
                    className="bg-surface-variant h-1.5 w-2/3 max-w-60 overflow-hidden rounded-full"
                    role="progressbar"
                    aria-label={m.profile_pending_mask_message()}
                  >
                    <div className="bg-primary h-full w-full animate-pulse rounded-full" />
                  </div>
                  <p className="text-on-surface-variant text-xs">
                    {m.profile_pending_mask_message()}
                  </p>
                </motion.div>
              )}
            </AnimatePresence>

            {isProxy && activeProfile.isActive && (
              <div
                className="from-primary/25 via-primary-container/20 absolute inset-0 bg-gradient-to-br to-transparent opacity-70"
                data-slot="profile-card-active-background"
              />
            )}

            <CardHeader
              className="relative flex items-center justify-between gap-2"
              data-slot="profile-card-title"
            >
              <TextMarquee className="z-10 min-w-0 flex-1">
                {profile.name}
              </TextMarquee>
              {isProxy && activeProfile.isActive && (
                <Chip className="z-10 shrink-0">
                  {m.profile_is_active_label()}
                </Chip>
              )}
            </CardHeader>

            <CardContent className="relative">
              <Chip>{typeLabel}</Chip>
            </CardContent>

            <CardFooter className="relative flex-row items-center justify-between gap-2">
              <Button className="flex items-center justify-center" asChild>
                <Link
                  to="/main/profiles/$type/detail/$uid"
                  params={{ type, uid: profile.uid }}
                >
                  {m.profile_view_details_title()}
                </Link>
              </Button>
              {hasMaterializedFile(profile) && (
                <ProfileCardAction label={m.profile_view_content_title()}>
                  <ViewContent
                    profile={profile}
                    icon
                    className="size-9 shrink-0"
                    disabled={isPending}
                    data-slot="profile-card-view-content"
                    aria-label={m.profile_view_content_title()}
                  >
                    <EditSquareOutlineRounded className="size-5" aria-hidden />
                  </ViewContent>
                </ProfileCardAction>
              )}
            </CardFooter>
          </div>
        </Card>
      </RegisterContextMenuTrigger>

      <RegisterContextMenuContent>
        {isProxy && (
          <ContextMenuItem
            disabled={isPending}
            onClick={() => void activeProfile.handleClick()}
          >
            <DragClickRounded className="size-4" />
            <span>{m.profile_active_title()}</span>
          </ContextMenuItem>
        )}
        <ContextMenuItem
          disabled={isPending}
          onClick={() => void deleteProfile.handleClick()}
        >
          <DeleteForeverOutlineRounded className="size-4" />
          <span>{m.profile_delete_title()}</span>
        </ContextMenuItem>
      </RegisterContextMenuContent>
    </RegisterContextMenu>
  );
}
