import type { ProfileQueryResultItem } from '@chimera/interface';
import { ProfileType } from '../../_modules/consts';

export const isProxyProfile = (profile: ProfileQueryResultItem) =>
  profile.type === 'local' || profile.type === 'remote';

export const isJavaScriptProfile = (profile: ProfileQueryResultItem) =>
  profile.type === 'script' && profile.script_type === 'javascript';

export const isLuaProfile = (profile: ProfileQueryResultItem) =>
  profile.type === 'script' && profile.script_type === 'lua';

export const isMergeProfile = (profile: ProfileQueryResultItem) =>
  profile.type === 'merge';

export const categoryProfiles = (items: ProfileQueryResultItem[] = []) => ({
  [ProfileType.Profile]: items.filter(isProxyProfile),
  [ProfileType.JavaScript]: items.filter(isJavaScriptProfile),
  [ProfileType.Lua]: items.filter(isLuaProfile),
  [ProfileType.Merge]: items.filter(isMergeProfile),
});
