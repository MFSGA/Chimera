import { type ProfileQueryResultItem } from '@chimera/interface';
import { useLockFn } from '@/hooks/use-lock-fn';
import * as m from '@/paraglide/messages';
import { formatError } from '@/utils';
import { message } from '@/utils/notification';

/** Reveals the profile's file in the system file manager. */
export const useShowInFileManager = (profile: ProfileQueryResultItem) =>
  useLockFn(async () => {
    try {
      await profile.view?.();
    } catch (error) {
      await message(formatError(error), {
        title: m.profile_detail_show_in_file_manager(),
        kind: 'error',
      });
    }
  });
