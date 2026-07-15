import { useProfile } from '@chimera/interface';
import { createFileRoute } from '@tanstack/react-router';
import EditSquareOutlineRounded from '~icons/material-symbols/edit-square-outline-rounded';
import { Button } from '@/components/ui/button';
import TextMarquee from '@/components/ui/text-marquee';
import ActionCard from './_modules/action-card';
import DetailHeader from './_modules/detail-header';
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
  const profile = query.data?.items.find((item) => item.uid === uid);

  if (!profile) {
    return null;
  }

  return (
    <div className="flex min-h-0 flex-1 flex-col">
      <DetailHeader type={type}>
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
        <ActionCard type={type} profile={profile} />
      </div>
    </div>
  );
}
