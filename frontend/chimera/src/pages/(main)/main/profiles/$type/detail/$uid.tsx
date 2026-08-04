import { useProfile } from '@chimera/interface';
import { createFileRoute } from '@tanstack/react-router';
import EditSquareOutlineRounded from '~icons/material-symbols/edit-square-outline-rounded';
import { Button } from '@/components/ui/button';
import TextMarquee from '@/components/ui/text-marquee';
import * as m from '@/paraglide/messages';
import { parseProfileType } from '../../_modules/consts';
import ActionCard from './_modules/action-card';
import DetailHeader from './_modules/detail-header';
import { resolveProfileDetailState } from './_modules/profile-detail-state';
import ProfileNameEditor from './_modules/profile-name-editor';
import SubscriptionCard from './_modules/subscription-card';

export const Route = createFileRoute('/(main)/main/profiles/$type/detail/$uid')(
  {
    component: RouteComponent,
  },
);

function RouteComponent() {
  const { type, uid } = Route.useParams();
  const { query } = useProfile();
  const profileType = parseProfileType(type);
  if (!profileType) {
    return (
      <div
        className="text-on-surface-variant flex min-h-0 flex-1 items-center justify-center text-sm"
        data-slot="profile-type-unsupported"
      >
        {m.common_error()}: {type}
      </div>
    );
  }

  const state = resolveProfileDetailState(
    query.data?.items,
    uid,
    query.isPending,
  );

  if (state.status === 'loading') {
    return (
      <div
        className="text-on-surface-variant flex min-h-0 flex-1 items-center justify-center text-sm"
        data-slot="profile-detail-loading"
      >
        {m.common_loading()}
      </div>
    );
  }

  if (state.status === 'missing') {
    return (
      <div
        className="text-on-surface-variant flex min-h-0 flex-1 items-center justify-center text-sm"
        data-slot="profile-detail-missing"
      >
        {m.common_error()}: {m.profile_empty_list_message()}
      </div>
    );
  }

  const { profile } = state;

  return (
    <div className="flex min-h-0 flex-1 flex-col">
      <DetailHeader type={profileType}>
        <TextMarquee className="w-0 min-w-0 flex-1 text-lg font-bold">
          {profile.name}
        </TextMarquee>

        <ProfileNameEditor profile={profile} asChild>
          <Button icon className="shrink-0">
            <EditSquareOutlineRounded className="size-4" />
          </Button>
        </ProfileNameEditor>
      </DetailHeader>

      <div className="grid grid-cols-2 gap-4 p-4 md:grid-cols-4">
        {profile.type === 'remote' && <SubscriptionCard profile={profile} />}
        <ActionCard type={profileType} profile={profile} />
      </div>
    </div>
  );
}
