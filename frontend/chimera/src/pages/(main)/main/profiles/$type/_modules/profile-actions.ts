import {
  useClashConnections,
  useProfile,
  type ProfileQueryResultItem,
} from '@chimera/interface';
import { ask } from '@tauri-apps/plugin-dialog';
import { useBlockTask } from '@/components/providers/block-task-provider';
import { useLockFn } from '@/hooks/use-lock-fn';
import * as m from '@/paraglide/messages';
import { formatError } from '@/utils';
import { message } from '@/utils/notification';

export const useActiveProfile = (profile: ProfileQueryResultItem) => {
  const { query, activate } = useProfile();
  const { deleteConnections } = useClashConnections();
  const isActive = query.data?.current === profile.uid;

  const task = useBlockTask(`active-profile-${profile.uid}`, async () => {
    try {
      await activate.mutateAsync(profile.uid);
      await deleteConnections.mutateAsync(undefined);
      await message(m.profile_active_title_success({ name: profile.name }), {
        title: m.profile_active_title(),
        kind: 'info',
      });
    } catch (error) {
      await message(
        m.profile_active_title_error({ name: profile.name }) +
          ` \n ${formatError(error)}`,
        { title: m.common_error(), kind: 'error' },
      );
    }
  });

  const handleClick = useLockFn(async () => {
    if (isActive) {
      await message(m.profile_is_active_description(), {
        title: m.profile_active_title(),
        kind: 'info',
      });
      return;
    }
    await task.execute();
  });

  return { isActive, isPending: task.isPending, handleClick };
};

export const useDeleteProfile = (profile: ProfileQueryResultItem) => {
  const { drop } = useProfile();
  const task = useBlockTask(`delete-profile-${profile.uid}`, async () => {
    try {
      await drop.mutateAsync(profile.uid);
    } catch (error) {
      await message(`${m.profile_delete_failed()} \n ${formatError(error)}`, {
        title: m.common_error(),
        kind: 'error',
      });
    }
  });

  const handleClick = useLockFn(async () => {
    const accepted = await ask(m.profile_delete_description(), {
      title: m.profile_delete_title(),
      kind: 'warning',
    });
    if (accepted) await task.execute();
  });

  return { isPending: task.isPending, handleClick };
};
