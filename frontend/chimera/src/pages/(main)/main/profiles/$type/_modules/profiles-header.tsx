import { cn } from '@chimera/ui';
import { Link } from '@tanstack/react-router';
import AccountTreeRounded from '~icons/material-symbols/account-tree-rounded';
import ArrowBackIosNewRounded from '~icons/material-symbols/arrow-back-ios-new-rounded';
import { Button } from '@/components/ui/button';
import useIsMobile from '@/hooks/use-is-moblie';
import * as m from '@/paraglide/messages';
import { ProfileType } from '../../_modules/consts';
import ProfileQuickImport from '../../_modules/profile-quick-import';
import { Route as IndexRoute } from '../index';
import ChainProfileImport from './chain-profile-import';
import TransformChainEditor from './transform-chain-editor';

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
        'flex items-center gap-2 p-4',
        'sticky top-0 z-50',
        'bg-mixed-background',
      )}
      data-slot="profiles-header"
    >
      {isMobile && <BackButton />}
      {isProfileType ? (
        <ProfileQuickImport />
      ) : (
        <div className="flex min-w-0 flex-1 items-center justify-between gap-2">
          <p className="truncate text-lg font-bold">
            {labels[type as ProfileType]}
          </p>
          <div className="flex shrink-0 items-center gap-2">
            <TransformChainEditor>
              <Button
                variant="fab"
                icon
                aria-label={m.profile_title_global_proxy_chains()}
                data-slot="global-transform-chain"
              >
                <AccountTreeRounded className="size-6" />
              </Button>
            </TransformChainEditor>
            <ChainProfileImport />
          </div>
        </div>
      )}
    </div>
  );
}
