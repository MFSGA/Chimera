export enum ProfileType {
  Profile = 'profile',
  JavaScript = 'javascript',
  Lua = 'lua',
  Merge = 'merge',
}

const PROFILE_TYPES = new Set<string>(Object.values(ProfileType));

export function parseProfileType(value: string): ProfileType | null {
  return PROFILE_TYPES.has(value) ? (value as ProfileType) : null;
}
