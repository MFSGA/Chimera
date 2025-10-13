import { useMutation, useQuery, useQueryClient } from '@tanstack/react-query';
import { unwrapResult } from '../utils';
import { commands } from './bindings';
import { RROFILES_QUERY_KEY } from './consts';

type URLImportParams = Parameters<typeof commands.importProfile>;

type CreateParams = {
  type: 'url';
  data: {
    url: URLImportParams[0];
    option: URLImportParams[1];
  };
};
/* todo | {
    type: 'manual'
    data: {
      item: ManualImportParams[0]
      fileData: ManualImportParams[1]
    }
  } */

/**
 * A custom hook for managing profiles with various operations including creation, updating, sorting, and deletion.
 *
 * @remarks
 * This hook provides comprehensive profile management functionality through React Query:
 * - Fetching profiles with optional helper functions
 * - Creating/importing profiles from URLs or files
 * - Updating existing profiles
 * - Reordering profiles
 * - Upserting profile configurations
 * - Deleting profiles
 *
 * Each operation automatically handles cache invalidation and refetching when successful.
 *
 * @param options - Configuration options for the hook
 * @param options.without_helper_fn - When true, disables the addition of helper functions to profile items
 *
 * @returns An object containing:
 * - query: Query result for fetching profiles
 * - create: Mutation for creating/importing profiles
 * - update: Mutation for updating existing profiles
 * - sort: Mutation for reordering profiles
 * - upsert: Mutation for upserting profile configurations
 * - drop: Mutation for deleting profiles
 *
 * @example
 * ```tsx
 * const { query, create, update, sort, upsert, drop } = useProfile();
 *
 * // Fetch profiles
 * const profiles = query.data?.items;
 *
 * // Create a new profile
 * create.mutate({ type: 'file', data: { item: newProfile, fileData: 'config' }});
 *
 * // Update a profile
 * update.mutate({ uid: 'profile-id', profile: updatedProfile });
 * ```
 */
export const useProfile = (options?: { without_helper_fn?: boolean }) => {
  const queryClient = useQueryClient();

  /**
   * Mutation hook for creating or importing profiles
   *
   * @remarks
   * This mutation handles two types of profile creation:
   * 1. URL-based import using `importProfile` command
   * 2. Direct creation using `createProfile` command
   *
   * @returns A mutation object that accepts CreateParams and handles profile creation
   *
   * @throws Will throw an error if the profile creation/import fails
   *
   * @example
   * ```ts
   * const { mutate } = create();
   * // Import from URL
   * mutate({ type: 'url', data: { url: 'https://example.com/config.yaml', option: {...} }});
   * // Create directly
   * mutate({ type: 'file', data: { item: {...}, fileData: '...' }});
   * ```
   */
  const create = useMutation({
    mutationFn: async ({ type, data }: CreateParams) => {
      if (type === 'url') {
        const { url, option } = data;
        return unwrapResult(await commands.importProfile(url, option));
      } else {
        // todo
        // const { item, fileData } = data
        // return unwrapResult(await commands.createProfile(item, fileData))
      }
    },
    onSuccess: () => {
      // Invalidate and refetch
      queryClient.invalidateQueries({ queryKey: [RROFILES_QUERY_KEY] });
    },
  });

  return { create };
};
