import type { ProfileQueryResultItem } from '@chimera/interface';
import { cn } from '@chimera/ui';
import { useNavigate } from '@tanstack/react-router';
import DeleteForeverOutlineRounded from '~icons/material-symbols/delete-forever-outline-rounded';
import DragClickRounded from '~icons/material-symbols/drag-click-rounded';
import EditSquareOutlineRounded from '~icons/material-symbols/edit-square-outline-rounded';
import FileOpenOutlineRounded from '~icons/material-symbols/file-open-outline-rounded';
import type { ComponentProps } from 'react';
import { useBlockTask } from '@/components/providers/block-task-provider';
import { Button } from '@/components/ui/button';
import { useLockFn } from '@/hooks/use-lock-fn';
import * as m from '@/paraglide/messages';
import { formatError } from '@/utils';
import { message } from '@/utils/notification';
import {
  useActiveProfile,
  useDeleteProfile,
} from '../../_modules/profile-actions';
import ProfileNameEditor from './profile-name-editor';
import SubscriptionUrlEditor from './subscription-url-editor';
import ViewContent from './view-content';

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
  type,
  profile,
}: {
  type: string;
  profile: ProfileQueryResultItem;
}) {
  const navigate = useNavigate();
  const active = useActiveProfile(profile);
  const deletion = useDeleteProfile(profile, {
    onSuccess: async () => {
      await navigate({ to: '/main/profiles/$type', params: { type } });
    },
  });
  const openTask = useBlockTask(`open-profile-${profile.uid}`, async () => {
    try {
      await profile.view?.();
    } catch (error) {
      await message(formatError(error), {
        title: m.common_error(),
        kind: 'error',
      });
    }
  });
  const openLocally = useLockFn(openTask.execute);
  const isPending = active.isPending || deletion.isPending;

  return (
    <div className="col-span-2 grid grid-cols-2 gap-4">
      <ProfileNameEditor profile={profile} asChild>
        <ActionButton>
          <EditSquareOutlineRounded className="size-4 shrink-0" />
          <span className="truncate">{m.profile_name_editor_title()}</span>
        </ActionButton>
      </ProfileNameEditor>

      {profile.type === 'remote' && (
        <SubscriptionUrlEditor profile={profile} asChild>
          <ActionButton>
            <EditSquareOutlineRounded className="size-4 shrink-0" />
            <span className="truncate">
              {m.profile_subscription_url_editor_label()}
            </span>
          </ActionButton>
        </SubscriptionUrlEditor>
      )}

      <ActionButton
        disabled={isPending}
        loading={active.isPending}
        onClick={() => void active.handleClick()}
      >
        <DragClickRounded className="size-4 shrink-0" />
        <span className="truncate">{m.profile_active_title()}</span>
      </ActionButton>

      <ActionButton
        disabled={isPending}
        loading={deletion.isPending}
        onClick={() => void deletion.handleClick()}
      >
        <DeleteForeverOutlineRounded className="size-4 shrink-0" />
        <span className="truncate">{m.profile_delete_title()}</span>
      </ActionButton>

      <ViewContent profile={profile} asChild>
        <ActionButton disabled={isPending}>
          <FileOpenOutlineRounded className="size-4 shrink-0" />
          <span className="truncate">{m.profile_view_content_title()}</span>
        </ActionButton>
      </ViewContent>

      <ActionButton
        disabled={isPending}
        loading={openTask.isPending}
        onClick={() => void openLocally()}
      >
        <FileOpenOutlineRounded className="size-4 shrink-0" />
        <span className="truncate">{m.profile_open_locally_title()}</span>
      </ActionButton>
    </div>
  );
}
