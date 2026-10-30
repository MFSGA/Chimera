import { useProfile, type ProfileQueryResultItem } from '@chimera/interface';
import { ask } from '@tauri-apps/plugin-dialog';
import { useBlockTask } from '@/components/providers/block-task-provider';
import { useLockFn } from '@/hooks/use-lock-fn';
import * as m from '@/paraglide/messages';
import { formatError } from '@/utils';
import { message } from '@/utils/notification';

export const useDeleteProfile = (
  profile: ProfileQueryResultItem,
  options?: {
    description?: string;
    onSuccess?: () => void | Promise<void>;
  },
) => {
  const { drop } = useProfile();

  const blockTask = useBlockTask(`delete-profile-${profile.uid}`, async () => {
    try {
      await drop.mutateAsync(profile.uid);
      await options?.onSuccess?.();
    } catch (error) {
      await message(`${m.profile_delete_failed()} \n ${formatError(error)}`, {
        title: m.common_error(),
        kind: 'error',
      });
    }
  });

  const handleClick = useLockFn(async () => {
    const accepted = await ask(
      options?.description ?? m.profile_delete_description(),
      { title: m.profile_delete_title(), kind: 'warning' },
    );
    if (!accepted) return;
    await blockTask.execute();
  });

  return { handleClick, isPending: blockTask.isPending };
};
