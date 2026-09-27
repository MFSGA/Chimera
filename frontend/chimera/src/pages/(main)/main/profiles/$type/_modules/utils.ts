import {
  isConfigItem,
  isTransformItem,
  type ProfileQueryResultItem,
} from '@chimera/interface';
import { ProfileType } from '../../_modules/consts';

export const isProxyProfile = (profile: ProfileQueryResultItem) =>
  isConfigItem(profile);

export const isJavaScriptProfile = (profile: ProfileQueryResultItem) =>
  isTransformItem(profile) &&
  profile.transform.type === 'script' &&
  profile.transform.runtime === 'javascript';

export const isLuaProfile = (profile: ProfileQueryResultItem) =>
  isTransformItem(profile) &&
  profile.transform.type === 'script' &&
  profile.transform.runtime === 'lua';

export const isMergeProfile = (profile: ProfileQueryResultItem) =>
  isTransformItem(profile) && profile.transform.type === 'overlay';

export const categoryProfiles = (items: ProfileQueryResultItem[] = []) => ({
  [ProfileType.Profile]: items.filter(isProxyProfile),
  [ProfileType.JavaScript]: items.filter(isJavaScriptProfile),
  [ProfileType.Lua]: items.filter(isLuaProfile),
  [ProfileType.Merge]: items.filter(isMergeProfile),
});
