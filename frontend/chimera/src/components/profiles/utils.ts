import type { Profile } from '@chimera/interface';

/**
 * Filters an array of profiles into two categories: clash and chain profiles.
 *
 * @param items - Array of Profile objects to be filtered
 * @returns An object containing two arrays:
 *          - clash: Array of profiles where type is 'remote' or 'local'
 *          - chain: Array of profiles where type is 'merge' or has a script property
 */
export function filterProfiles<T extends Profile>(items?: T[]) {
  /**
   * Filters the input array to include only items of type 'remote' or 'local'
   * @param items - Array of items to filter
   * @returns {Array} Filtered array containing only remote and local items
   */
  const clash = items?.filter(
    (item) => item.type === 'remote' || item.type === 'local',
  );

  /**
   * Filters an array of items to get a chain of either 'merge' type items
   * or items with a script property in their type object.
   *
   * @param {Array<{ type: string | { script: 'javascript' | 'lua' } }>} items - The array of items to filter
   * @returns {Array<{ type: string | { script: 'javascript' | 'lua' } }>} A filtered array containing only merge items or items with scripts
   */
  /* todo
  const chain = items?.filter(
    (item) => item.type === 'merge' || item.type === 'script',
  ); */

  return {
    clash,
    // chain,
  };
}

export type ClashProfile = Extract<Profile, { type: 'remote' | 'local' }>;

export type ClashProfileBuilder = Extract<
  ProfileBuilder,
  { type: 'remote' | 'local' }
>;
