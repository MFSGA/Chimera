import { type ProfileQueryResultItem } from '@chimera/interface';
import { cn } from '@chimera/ui';
import { Button } from '@chimera/ui/button';
import AccountTreeRounded from '~icons/material-symbols/account-tree-rounded';
import DragClickRounded from '~icons/material-symbols/drag-click-rounded';
import EditSquareOutlineRounded from '~icons/material-symbols/edit-square-outline-rounded';
import type { ComponentProps } from 'react';
import * as m from '@/paraglide/messages';
import { useActiveProfile } from '../../../_modules/use-active-profile';
import { isProxyProfile } from '../../../_modules/utils';
import ProfileNameEditor from './profile-name-editor';

const ActionButton = ({
  className,
  ...props
}: ComponentProps<typeof Button>) => (
  <Button
    variant="basic"
    className={cn(
      'bg-primary-container dark:bg-surface-variant/30 flex h-14 items-center gap-3 truncate rounded-2xl text-base font-semibold',
      className,
    )}
    {...props}
  />
);

export default function ActionCard({
  profile,
  onOpenTransforms,
}: {
  profile: ProfileQueryResultItem;
  onOpenTransforms: () => void;
}) {
  const active = useActiveProfile(profile);
  const isProxy = isProxyProfile(profile);
  const isPending = active.isPending;

  return (
    <div className="col-span-2 grid grid-cols-2 gap-4">
      <ProfileNameEditor profile={profile} asChild>
        <ActionButton>
          <EditSquareOutlineRounded className="size-4 shrink-0" />
          <span className="truncate">{m.profile_name_editor_title()}</span>
        </ActionButton>
      </ProfileNameEditor>

      {isProxy && (
        <ActionButton
          disabled={isPending}
          loading={active.isPending}
          onClick={() => void active.handleClick()}
        >
          <DragClickRounded className="size-4 shrink-0" />
          <span className="truncate">{m.profile_active_title()}</span>
        </ActionButton>
      )}

      {isProxy && (
        <ActionButton
          disabled={isPending}
          data-slot="profile-transform-chain"
          data-profile-uid={profile.uid}
          onClick={onOpenTransforms}
        >
          <AccountTreeRounded className="size-4 shrink-0" />
          <span className="truncate">{m.profile_menu_proxy_chains()}</span>
        </ActionButton>
      )}
    </div>
  );
}
