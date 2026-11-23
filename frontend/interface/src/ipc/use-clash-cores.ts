import { useMutation, useQuery, useQueryClient } from '@tanstack/react-query';
import { kebabCase } from 'lodash-es';
import { unwrapResult } from '../utils';
import {
  commands,
  type ClashCore_Deserialize,
  type ClashCore_Serialize,
} from './bindings';
import {
  CHIMERA_SETTING_QUERY_KEY,
  CLASH_CORE_QUERY_KEY,
  CLASH_CORE_STATUS_QUERY_KEY,
  CLASH_VERSION_QUERY_KEY,
  CORE_ACTOR_STATUS_QUERY_KEY,
} from './consts';
import { restartCoreSidecar } from './core-mutation';

export const ClashCores = {
  // clash: 'Clash Premium',
  mihomo: 'Mihomo',
  // 'mihomo-alpha': 'Mihomo Alpha',
  'clash-rs': 'Clash Rust',
  'chimera-client': 'Chimera Client',
  'chimera-client-alpha': 'Chimera Client Alpha',
  'clash-rs-alpha': 'Clash Rust Alpha',
} satisfies Partial<Record<ClashCore_Serialize, string>>;

type SupportedClashCore = keyof typeof ClashCores;

export type ClashCoresInfo = Record<ClashCore_Serialize, ClashCoresDetail>;

export type ClashCoresDetail = {
  name: string;
  currentVersion: string;
  latestVersion?: string;
};

export const useClashCores = () => {
  const queryClient = useQueryClient();

  // Both old two-state consumers and ref CoreActor readers need to observe
  // a completed (or uncertain) lifecycle mutation. Never fabricate Running
  // from the requested operation; ref CoreActor is the status authority.
  const refreshCoreStatus = () => {
    void queryClient.invalidateQueries({
      queryKey: [CORE_ACTOR_STATUS_QUERY_KEY],
    });
    void queryClient.invalidateQueries({
      queryKey: [CLASH_CORE_STATUS_QUERY_KEY],
    });
  };

  const query = useQuery({
    queryKey: [CLASH_CORE_QUERY_KEY],
    queryFn: async () => {
      return await Object.keys(ClashCores).reduce(
        async (acc, key) => {
          const result = await acc;
          try {
            const currentVersion =
              unwrapResult(
                await commands.getCoreVersion(key as ClashCore_Deserialize),
              ) ?? 'N/A';

            result[key as ClashCore_Serialize] = {
              name: ClashCores[key as SupportedClashCore],
              currentVersion,
            };
          } catch (e) {
            console.error('failed to fetch core version', e);
            result[key as ClashCore_Serialize] = {
              name: ClashCores[key as SupportedClashCore],
              currentVersion: 'N/A',
            };
          }
          return result;
        },
        Promise.resolve({} as ClashCoresInfo),
      );
    },
  });

  const fetchRemote = useMutation({
    mutationFn: async () => {
      const results = unwrapResult(await commands.fetchLatestCoreVersions());

      if (!results) {
        return;
      }

      const currentData = queryClient.getQueryData([
        CLASH_CORE_QUERY_KEY,
      ]) as ClashCoresInfo;

      if (currentData && results) {
        const updatedData = { ...currentData };

        Object.entries(results).forEach(([_key, latestVersion]) => {
          const key = kebabCase(_key);

          if (updatedData[key as ClashCore_Serialize]) {
            updatedData[key as ClashCore_Serialize] = {
              ...updatedData[key as ClashCore_Serialize],
              latestVersion,
            };
          }
        });

        queryClient.setQueryData([CLASH_CORE_QUERY_KEY], updatedData);
      }
      return results;
    },
  });

  const updateCore = useMutation({
    mutationFn: async (core: ClashCore_Deserialize) => {
      return unwrapResult(await commands.updateCore(core));
    },
    onSuccess: () => {
      queryClient.invalidateQueries({ queryKey: [CLASH_CORE_QUERY_KEY] });
    },
  });

  const upsert = useMutation({
    mutationFn: async (core: ClashCore_Deserialize) => {
      return unwrapResult(await commands.changeClashCore(core));
    },
    onSuccess: () => {
      void queryClient.invalidateQueries({ queryKey: [CLASH_CORE_QUERY_KEY] });
      void queryClient.invalidateQueries({
        queryKey: [CHIMERA_SETTING_QUERY_KEY],
      });
      void queryClient.invalidateQueries({
        queryKey: ['getConfigFeatures'],
      });
      void queryClient.invalidateQueries({
        queryKey: [CLASH_VERSION_QUERY_KEY],
      });
    },
    // An error may occur after the CoreActor admits a mutation. Refresh the
    // authoritative state on both success and failure, not just on success.
    onSettled: refreshCoreStatus,
  });

  const restartSidecar = async (): Promise<void> => {
    // Unwrap the Tauri Result before notifying success. A rejected command
    // still invalidates both ref and legacy projections via onSettled.
    await restartCoreSidecar(commands, refreshCoreStatus);
  };

  return {
    query,
    updateCore,
    upsert,
    restartSidecar,
    fetchRemote,
  };
};
