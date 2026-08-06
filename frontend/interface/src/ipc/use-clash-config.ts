import { useMutation, useQuery, useQueryClient } from '@tanstack/react-query';
import { unwrapResult } from '../utils';
import { ClashRuntimeConfig, commands, PatchRuntimeConfig } from './bindings';
import { CLASH_CONFIG_QUERY_KEY } from './consts';

/**
 * A hook that manages fetching and updating the Clash configuration.
 *
 * @remarks
 * This hook fetches the current Clash configuration using a query keyed by `['clash-config']`
 * and allows updates via an upsert mutation. The upsert mutation:
 * - Patches the core and application configuration through `commands.patchClashConfig`.
 * - On success, it invalidates the `['clash-config']` query so the UI reads the
 *   value returned by the running core.
 *
 * @returns An object with:
 * - `query`: The result of the useQuery hook that retrieves the current configuration.
 * - `upsert`: The mutation object that can be used to update the configuration.
 *
 * @example
 * const { query, upsert } = useClashConfig();
 */
export const useClashConfig = () => {
  const queryClient = useQueryClient();

  /**
   * Retrieves the Clash configuration using a query.
   *
   * @remarks
   * The query is configured with the key 'clash-config' and uses the
   * getConfigs function as its query function. This setup ensures that:
   * - The data is uniquely identified and cached based on the query key.
   * - The asynchronous retrieval of configuration data is handled
   *   via the getConfigs function.
   *
   * @see useQuery - For additional configuration options and usage details.
   */
  const query = useQuery<ClashRuntimeConfig | undefined>({
    queryKey: [CLASH_CONFIG_QUERY_KEY],
    queryFn: async () => unwrapResult(await commands.clashApiGetConfigs()),
  });

  /**
   * Performs an upsert operation to update or insert the Clash configuration.
   *
   * This mutation accepts only the persistent runtime override DTO. It does not
   * accept a running-core snapshot, keeping desired state separate from the
   * `GET /configs` response.
   *
   * @returns A Promise resolving to the updated configuration, obtained by unwrapping the result of the
   *          commands.patchClashConfig call.
   */
  const upsert = useMutation({
    mutationFn: async (payload: PatchRuntimeConfig) => {
      return unwrapResult(await commands.patchClashConfig(payload));
    },
    onSuccess: () => {
      return queryClient.invalidateQueries({
        queryKey: [CLASH_CONFIG_QUERY_KEY],
      });
    },
  });

  return {
    query,
    upsert,
  };
};
