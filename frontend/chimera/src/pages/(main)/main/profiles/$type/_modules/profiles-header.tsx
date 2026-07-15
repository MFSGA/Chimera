import { cn } from '@chimera/ui';
import { Link } from '@tanstack/react-router';
import ArrowBackIosNewRounded from '~icons/material-symbols/arrow-back-ios-new-rounded';
import { QuickImport } from '@/components/profiles/quick-import';
import { Button } from '@/components/ui/button';
import useIsMobile from '@/hooks/use-is-moblie';
import * as m from '@/paraglide/messages';
import { ProfileType } from '../../_modules/consts';
import { Route as IndexRoute } from '../index';

const BackButton = () => (
  <Button icon className="flex items-center justify-center md:hidden" asChild>
    <Link to="/main/profiles">
      <ArrowBackIosNewRounded className="size-4" />
    </Link>
  </Button>
);

export default function ProfilesHeader() {
  const { type } = IndexRoute.useParams();
  const isMobile = useIsMobile();
  const isProfileType = type === ProfileType.Profile;
  const labels = {
    [ProfileType.Profile]: m.profile_profile_label(),
    [ProfileType.JavaScript]: m.profile_javascript_label(),
    [ProfileType.Lua]: m.profile_lua_label(),
    [ProfileType.Merge]: m.profile_merge_label(),
  } satisfies Record<ProfileType, string>;

  return (
    <div
      className={cn(
        'bg-mixed-background sticky top-0 z-10 flex items-center gap-2 p-4',
      )}
      data-slot="profiles-header"
    >
      {isMobile && <BackButton />}
      {isProfileType ? (
        <QuickImport />
      ) : (
        <p className="text-lg font-bold">{labels[type as ProfileType]}</p>
      )}
    </div>
  );
}
