import type { ProfileQueryResultItem } from '@chimera/interface';
import { ProfileType } from '../../_modules/consts';

export const isProxyProfile = (profile: ProfileQueryResultItem) =>
  profile.type === 'local' || profile.type === 'remote';

export const categoryProfiles = (items: ProfileQueryResultItem[] = []) => ({
  [ProfileType.Profile]: items.filter(isProxyProfile),
  // Chimera's current profile IPC only serializes local/remote config profiles.
  // Keep the ref tabs and layout in main UI, but represent unsupported transform
  // profile categories as empty until the backend exposes them.
  [ProfileType.JavaScript]: [] as ProfileQueryResultItem[],
  [ProfileType.Lua]: [] as ProfileQueryResultItem[],
  [ProfileType.Merge]: [] as ProfileQueryResultItem[],
});
