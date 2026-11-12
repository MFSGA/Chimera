import { getRemoteSource, isConfigItem, useProfile } from '@chimera/interface';
import { Button } from '@chimera/ui/button';
import { EmptyState } from '@chimera/ui/empty-state';
import { ScrollArea } from '@chimera/ui/scroll-area';
import { createFileRoute, Link, useNavigate } from '@tanstack/react-router';
import { AnimatePresence } from 'motion/react';
import { z } from 'zod';
import useIsMobile from '@/hooks/use-is-moblie';
import * as m from '@/paraglide/messages';
import { PROFILE_TYPE_NAMES, ProfileType } from '../../_modules/consts';
import { isChainProfile } from '../../_modules/utils';
import ActionCard from './_modules/action-card';
import DeleteSection from './_modules/delete-section';
import { DetailPanelSlot } from './_modules/detail-panel';
import FileSection from './_modules/file-section';
import ProfileDetailHeader from './_modules/profile-detail-header';
import ProfileDetailSkeleton from './_modules/profile-detail-skeleton';
import SubscriptionSection from './_modules/subscription-section';
import SyncHistorySection from './_modules/sync-history-section';
import TransformChainSection from './_modules/transform-chain-section';
import TransformsPanel from './_modules/transforms-panel';
import UsedBySection from './_modules/used-by-section';

export const Route = createFileRoute('/(main)/main/profiles/$type/detail/$uid')(
  {
    validateSearch: z.object({
      panel: z.enum(['transforms']).optional().catch(undefined),
    }),
    component: RouteComponent,
  },
);

function RouteComponent() {
  const { type, uid } = Route.useParams();
  const { panel } = Route.useSearch();
  const navigate = useNavigate({ from: Route.fullPath });
  const isMobile = useIsMobile();
  const layout = isMobile ? 'narrow' : 'wide';
  const { query } = useProfile();
  const profile = query.data?.items.find((item) => item.uid === uid);

  const closePanel = () =>
    navigate({
      search: (previous) => ({ ...previous, panel: undefined }),
    });

  if (query.isPending) {
    return (
      <div
        className="flex min-h-0 flex-1 flex-col px-4 pb-8"
        data-slot="profile-detail-loading"
      >
        <ProfileDetailSkeleton />
      </div>
    );
  }

  if (!profile) {
    return (
      <div
        className="flex min-h-0 flex-1 flex-col px-4 pb-8"
        data-slot="profile-detail-not-found"
      >
        <EmptyState className="flex-1" message={m.profile_detail_not_found()}>
          <Button variant="stroked" asChild>
            <Link to="/main/profiles/$type" params={{ type }}>
              {m.profile_detail_back({
                destination: PROFILE_TYPE_NAMES[type as ProfileType],
              })}
            </Link>
          </Button>
        </EmptyState>
      </div>
    );
  }

  const transformsPanelOpen = panel === 'transforms' && isConfigItem(profile);

  return (
    <div className="relative flex min-h-0 min-w-0 flex-1 overflow-hidden">
      <ScrollArea
        className="min-h-0 min-w-0 flex-1"
        inert={transformsPanelOpen && layout === 'narrow'}
      >
        <div
          className="flex min-h-0 flex-col"
          data-slot="profile-detail"
          data-profile-uid={profile.uid}
        >
          <ProfileDetailHeader type={type as ProfileType} profile={profile} />

          <div className="grid grid-cols-2 gap-4 p-4 md:grid-cols-4">
            {getRemoteSource(profile) && (
              <SubscriptionSection profile={profile} />
            )}
            {getRemoteSource(profile) && <SyncHistorySection uid={uid} />}
            <ActionCard
              profile={profile}
              onOpenTransforms={() =>
                navigate({
                  search: (previous) => ({ ...previous, panel: 'transforms' }),
                })
              }
            />
            {isConfigItem(profile) && (
              <TransformChainSection
                profile={profile}
                selected={transformsPanelOpen}
                onOpen={() =>
                  navigate({
                    search: (previous) => ({
                      ...previous,
                      panel: 'transforms',
                    }),
                  })
                }
              />
            )}
          </div>

          <div className="flex flex-col gap-4 px-4 pb-4">
            {isChainProfile(profile) && (
              <UsedBySection
                uid={profile.uid}
                profiles={query.data?.items ?? []}
                globalTransforms={query.data?.global_transforms ?? []}
              />
            )}
            <FileSection profile={profile} />
            <DeleteSection
              profile={profile}
              onDeleted={() =>
                navigate({ to: '/main/profiles/$type', params: { type } })
              }
            />
          </div>
        </div>
      </ScrollArea>

      <AnimatePresence initial={false}>
        {transformsPanelOpen && (
          <DetailPanelSlot layout={layout}>
            <TransformsPanel
              key={profile.uid}
              profile={profile}
              layout={layout}
              onClose={closePanel}
            />
          </DetailPanelSlot>
        )}
      </AnimatePresence>
    </div>
  );
}
