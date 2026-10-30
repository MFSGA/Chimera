import { getRemoteSource, isConfigItem, useProfile } from '@chimera/interface';
import { Button } from '@chimera/ui/button';
import { EmptyState } from '@chimera/ui/empty-state';
import { createFileRoute, Link } from '@tanstack/react-router';
import * as m from '@/paraglide/messages';
import { PROFILE_TYPE_NAMES, ProfileType } from '../../_modules/consts';
import ActionCard from './_modules/action-card';
import ChianEditorCard from './_modules/chian-editor-card';
import ProfileDetailHeader from './_modules/profile-detail-header';
import ProfileDetailSkeleton from './_modules/profile-detail-skeleton';
import SubscriptionCard from './_modules/subscription-card';
import { SyncHistoryCard } from './_modules/sync-history-card';

export const Route = createFileRoute('/(main)/main/profiles/$type/detail/$uid')(
  {
    component: RouteComponent,
  },
);

function RouteComponent() {
  const { type, uid } = Route.useParams();
  const { query } = useProfile();
  const profile = query.data?.items.find((item) => item.uid === uid);

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

  return (
    <div
      className="flex min-h-0 flex-1 flex-col"
      data-slot="profile-detail"
      data-profile-uid={profile.uid}
    >
      <ProfileDetailHeader type={type as ProfileType} profile={profile} />

      <div className="grid grid-cols-2 gap-4 p-4 md:grid-cols-4">
        {getRemoteSource(profile) && <SubscriptionCard profile={profile} />}
        {getRemoteSource(profile) && <SyncHistoryCard uid={uid} />}
        <ActionCard type={type} profile={profile} />
        {isConfigItem(profile) && <ChianEditorCard profile={profile} />}
      </div>
    </div>
  );
}
