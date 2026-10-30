import {
  useClashConnections,
  useProfile,
  type ProfileQueryResultItem,
} from '@chimera/interface';
import { useBlockTask } from '@/components/providers/block-task-provider';
import { useLockFn } from '@/hooks/use-lock-fn';
import * as m from '@/paraglide/messages';
import { formatError } from '@/utils';
import { message } from '@/utils/notification';

export const useActiveProfile = (profile: ProfileQueryResultItem) => {
  const { query, activate } = useProfile();
  const isActive = query.data?.current === profile.uid;
  const { deleteConnections } = useClashConnections();

  const blockTask = useBlockTask(`active-profile-${profile.uid}`, async () => {
    try {
      await activate.mutateAsync(profile.uid);
    } catch (error) {
      await message(
        `${m.profile_active_title_error({ name: profile.name })} \n ${formatError(error)}`,
        { title: m.common_error(), kind: 'error' },
      );
      return;
    }

    // Ref behavior: a failed connection cleanup must not report the
    // successful profile activation as a failed activation.
    try {
      await deleteConnections.mutateAsync(undefined);
    } catch (error) {
      console.warn('[active-profile] failed to delete connections:', error);
    }

    await message(m.profile_active_title_success({ name: profile.name }), {
      title: m.profile_active_title(),
      kind: 'info',
    });
  });

  const handleClick = useLockFn(async () => {
    if (isActive) {
      await message(m.profile_is_active_description(), {
        title: m.profile_active_title(),
        kind: 'info',
      });
      return;
    }

    await blockTask.execute();
  });

  return { isActive, handleClick, isPending: blockTask.isPending };
};
