import {
  isConfigItem,
  isTransformItem,
  type ProfileQueryResultItem,
} from '@chimera/interface';
import { ProfileType } from '../../_modules/consts';

export type ConfigProfile = Extract<ProfileQueryResultItem, { type: 'config' }>;
export type TransformProfile = Extract<
  ProfileQueryResultItem,
  { type: 'transform' }
>;

/** Chain candidate = Transform definition (Overlay / Script). */
export const isChainProfile = (
  profile: ProfileQueryResultItem,
): profile is TransformProfile => isTransformItem(profile);

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

/** The `$type` route segment a profile's detail page lives under. */
export const profileTypeOf = (profile: ProfileQueryResultItem): ProfileType => {
  if (isJavaScriptProfile(profile)) return ProfileType.JavaScript;
  if (isLuaProfile(profile)) return ProfileType.Lua;
  if (isMergeProfile(profile)) return ProfileType.Merge;
  return ProfileType.Profile;
};
