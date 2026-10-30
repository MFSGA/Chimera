import { getRemoteSource, isConfigItem, useProfile } from '@chimera/interface';
import { createFileRoute } from '@tanstack/react-router';
import { ProfileType } from '../../_modules/consts';
import ActionCard from './_modules/action-card';
import ChianEditorCard from './_modules/chian-editor-card';
import ProfileDetailHeader from './_modules/profile-detail-header';
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

  if (!profile) {
    return null;
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
