import {
  useQueryClient,
  type InvalidateQueryFilters,
  type QueryKey,
} from '@tanstack/react-query';
import { listen } from '@tauri-apps/api/event';
import { useCallback, useEffect, useMemo, type PropsWithChildren } from 'react';
import {
  CHIMERA_BACKEND_EVENT_NAME,
  CHIMERA_SETTING_QUERY_KEY,
  CHIMERA_SYSTEM_PROXY_QUERY_KEY,
  CLASH_CONFIG_QUERY_KEY,
  CLASH_INFO_QUERY_KEY,
  CLASH_PROXIES_PROVIDER_QUERY_KEY,
  CLASH_PROXIES_QUERY_KEY,
  CLASH_RULES_PROVIDER_QUERY_KEY,
  CLASH_RULES_QUERY_KEY,
  CLASH_SETTINGS_QUERY_KEY,
  CLASH_VERSION_QUERY_KEY,
  PROFILES_QUERY_KEY,
  RUNTIME_TRANSFORM_DIAGNOSTICS_QUERY_KEY,
} from '../ipc/consts';
import { queries } from '../ipc/profile-bindings';

// Same internally tagged wire representation as ref's StateChangedEvent.
// Kept outside generated Specta output until its generator runs.
type StateChanged =
  | { kind: 'nyanpasu_config' }
  | { kind: 'clash_config' }
  | { kind: 'profiles'; files: string[] }
  | { kind: 'proxies' }
  | { kind: 'runtime_transform_diagnostics' };

const mutationKeys: Record<StateChanged['kind'], QueryKey[]> = {
  nyanpasu_config: [
    [CHIMERA_SETTING_QUERY_KEY],
    [CHIMERA_SYSTEM_PROXY_QUERY_KEY],
    [CLASH_PROXIES_QUERY_KEY],
  ],
  clash_config: [
    [CLASH_SETTINGS_QUERY_KEY],
    [CLASH_VERSION_QUERY_KEY],
    [CLASH_INFO_QUERY_KEY],
    [CLASH_CONFIG_QUERY_KEY],
    [CLASH_PROXIES_QUERY_KEY],
    [CLASH_PROXIES_PROVIDER_QUERY_KEY],
    [CLASH_RULES_PROVIDER_QUERY_KEY],
    [CLASH_RULES_QUERY_KEY],
    [RUNTIME_TRANSFORM_DIAGNOSTICS_QUERY_KEY],
  ],
  profiles: [
    [PROFILES_QUERY_KEY],
    [CLASH_VERSION_QUERY_KEY],
    [CLASH_INFO_QUERY_KEY],
    [CLASH_PROXIES_QUERY_KEY],
    [CLASH_PROXIES_PROVIDER_QUERY_KEY],
    [CLASH_RULES_PROVIDER_QUERY_KEY],
    [CLASH_RULES_QUERY_KEY],
    [RUNTIME_TRANSFORM_DIAGNOSTICS_QUERY_KEY],
  ],
  proxies: [[CLASH_PROXIES_QUERY_KEY], [CLASH_PROXIES_PROVIDER_QUERY_KEY]],
  runtime_transform_diagnostics: [[RUNTIME_TRANSFORM_DIAGNOSTICS_QUERY_KEY]],
};

export const MutationProvider = ({ children }: PropsWithChildren) => {
  const queryClient = useQueryClient();

  const keptKeys = useMemo(
    () => [
      [CHIMERA_SETTING_QUERY_KEY],
      [CLASH_SETTINGS_QUERY_KEY],
      [PROFILES_QUERY_KEY],
      [CLASH_PROXIES_QUERY_KEY],
    ],
    [],
  );

  // Match ref: choose the cache policy before the children mount.
  useMemo(() => {
    for (const queryKey of keptKeys) {
      queryClient.setQueryDefaults(queryKey, { staleTime: Infinity });
    }
  }, [keptKeys, queryClient]);

  const refetch = useCallback(
    async (filters?: InvalidateQueryFilters) => {
      // A pre-commit read must not overwrite a freshly invalidated query.
      await queryClient.cancelQueries(filters);
      await queryClient.invalidateQueries(filters);
    },
    [queryClient],
  );

  const refetchQueries = useCallback(
    (keys: readonly QueryKey[]) => {
      Promise.all(keys.map((queryKey) => refetch({ queryKey }))).catch(
        console.error,
      );
    },
    [refetch],
  );

  useEffect(() => {
    let disposed = false;
    const stateChanges = listen<StateChanged>(
      CHIMERA_BACKEND_EVENT_NAME,
      ({ payload }) => {
        refetchQueries(mutationKeys[payload.kind]);
        if (payload.kind === 'profiles') {
          if (payload.files.length > 0) {
            refetchQueries(
              payload.files.map((uid) => queries.readProfileFile(uid).queryKey),
            );
          } else {
            // Current Chimera profile writers do not all report touched IDs yet.
            void refetch({ queryKey: ['readProfileFile'] }).catch(
              console.error,
            );
          }
        }
      },
    );

    // An event may commit while the asynchronous listener is being installed.
    stateChanges
      .then(() => {
        if (!disposed) refetchQueries(keptKeys);
      })
      .catch(console.error);

    return () => {
      disposed = true;
      void stateChanges.then((stop) => stop()).catch(console.error);
    };
  }, [keptKeys, refetch, refetchQueries]);

  return children;
};
