import {
  getRemoteSource,
  isConfigItem,
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
import InfoOutlineRounded from '~icons/material-symbols/info-outline-rounded';
import PowerSettingsNewRounded from '~icons/material-symbols/power-settings-new-rounded';
import { AnimatePresence, motion } from 'motion/react';
import { useRef } from 'react';
import {
  RegisterContextMenu,
  RegisterContextMenuContent,
  RegisterContextMenuTrigger,
} from '@/components/providers/context-menu-provider';
import * as m from '@/paraglide/messages';
import { Route as IndexRoute } from '../$type/index';
import { ActiveGradient } from './active-gradient';
import ProfileCardAction from './profile-card-action';
import ProfileCardFeatures, { ProfileCardChip } from './profile-card-features';
import ProfileCardSubscription, {
  ProfileCardTraffic,
  ProfileCardUpdate,
} from './profile-card-subscription';
import { ProfileMoveContextMenu, useProfileMove } from './profile-move-menu';
import { useActiveProfile } from './use-active-profile';
import { useDeleteProfile } from './use-delete-profile';
import { hasMaterializedFile, isProxyProfile } from './utils';
import ViewContent from './view-content';

export default function ProfileCard({
  profile,
  profiles,
  isGlobal,
}: {
  profile: ProfileQueryResultItem;
  profiles: ProfileQueryResultItem[];
  isGlobal: boolean;
}) {
  const { type } = IndexRoute.useParams();
  const activeProfile = useActiveProfile(profile);
  const deleteProfile = useDeleteProfile(profile);
  const isPending = activeProfile.isPending || deleteProfile.isPending;
  const isProxy = isProxyProfile(profile);
  const remote = getRemoteSource(profile);
  const featured = isProxy && activeProfile.isActive;
  const cardRef = useRef<HTMLDivElement>(null);
  const moveToFolder = useProfileMove(profile.uid);

  return (
    <RegisterContextMenu>
      <RegisterContextMenuTrigger asChild>
        <Card
          className={cn(
            'relative flex min-h-40 flex-col justify-between overflow-hidden',
            // The desktop Profile layout contract caps card height at 260px.
            // New subscription metrics scroll independently of the actions.
            isProxy && 'h-60',
          )}
          data-slot="profile-card"
          data-profile-uid={profile.uid}
          data-profile-active={String(activeProfile.isActive)}
          asChild
        >
          <div ref={cardRef}>
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

            {activeProfile.isActive && <ActiveGradient cardRef={cardRef} />}

            <CardHeader
              className="relative flex shrink-0 items-center justify-between gap-2"
              data-slot="profile-card-title"
            >
              <TextMarquee className="z-10 min-w-0 flex-1">
                {profile.name}
              </TextMarquee>
              {isProxy && activeProfile.isActive && (
                <ProfileCardChip className="z-10 shrink-0">
                  {m.profile_is_active_label()}
                </ProfileCardChip>
              )}
            </CardHeader>

            <CardContent className="relative flex min-h-0 flex-1 flex-col gap-3 overflow-y-auto">
              <ProfileCardFeatures
                profile={profile}
                profiles={profiles}
                isGlobal={isGlobal}
                compact={isProxy && !featured}
              />
              {remote ? (
                <ProfileCardSubscription
                  profile={profile}
                  featured={featured}
                />
              ) : isConfigItem(profile) ? (
                <ProfileCardTraffic profile={profile} featured={featured} />
              ) : null}
            </CardContent>

            <CardFooter className="relative flex shrink-0 flex-row items-center justify-between gap-1">
              {isProxy && !activeProfile.isActive ? (
                <Button
                  variant="stroked"
                  className="flex shrink-0 items-center gap-2 px-3 text-xs"
                  disabled={isPending}
                  data-slot="profile-card-activate"
                  aria-label={m.profile_active_title()}
                  onClick={() => void activeProfile.handleClick()}
                >
                  <PowerSettingsNewRounded className="size-4" aria-hidden />
                  {m.profile_active_title()}
                </Button>
              ) : (
                <span />
              )}

              <div className="flex shrink-0 items-center gap-1">
                {remote && (
                  <ProfileCardUpdate profile={profile} disabled={isPending} />
                )}
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
                      <EditSquareOutlineRounded
                        className="size-5"
                        aria-hidden
                      />
                    </ViewContent>
                  </ProfileCardAction>
                )}
                <ProfileCardAction label={m.profile_view_details_title()}>
                  <Button icon className="size-9 shrink-0" asChild>
                    <Link
                      to="/main/profiles/$type/detail/$uid"
                      params={{ type, uid: profile.uid }}
                      data-slot="profile-card-details"
                      aria-label={m.profile_view_details_title()}
                    >
                      <InfoOutlineRounded className="size-5" aria-hidden />
                    </Link>
                  </Button>
                </ProfileCardAction>
              </div>
            </CardFooter>
          </div>
        </Card>
      </RegisterContextMenuTrigger>

      <RegisterContextMenuContent>
        {isProxy && (
          <ProfileMoveContextMenu move={moveToFolder} disabled={isPending} />
        )}
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
