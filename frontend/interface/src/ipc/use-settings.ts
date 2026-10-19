import { useMutation, useQuery, useQueryClient } from '@tanstack/react-query';
import { unwrapResult } from '../utils';
import {
  commands,
  type ChimeraAppConfig_Serialize,
  type ChimeraAppConfigPatch_Serialize,
  type ClashConfig,
  type ClashConfigPatch_Serialize,
} from './bindings';
import { CHIMERA_SETTING_QUERY_KEY, CLASH_SETTINGS_QUERY_KEY } from './consts';

type ConfigPatchFn<TPatch> = (patch: TPatch) => Promise<unknown>;

const useTypedConfig = <TConfig, TPatch>(
  queryKey: readonly unknown[],
  queryFn: () => Promise<TConfig>,
  patchConfig: ConfigPatchFn<TPatch>,
) => {
  const queryClient = useQueryClient();
  const query = useQuery({ queryKey, queryFn });
  const upsert = useMutation({
    mutationFn: patchConfig,
    onSuccess: () => {
      queryClient.invalidateQueries({ queryKey });
    },
  });

  return { query, upsert };
};

const useTypedConfigField = <
  TConfig,
  TPatch extends object,
  K extends keyof TConfig & keyof TPatch,
  TClearable extends keyof TPatch = never,
>(
  { query, upsert: update }: ReturnType<typeof useTypedConfig<TConfig, TPatch>>,
  key: K,
) => {
  const data = query.data;
  const value = data?.[key];
  const upsert = async (
    value: K extends TClearable
      ? Exclude<TPatch[K], undefined>
      : NonNullable<TPatch[K]>,
  ) => {
    if (!data) return;
    await update.mutateAsync({ [key]: value } as TPatch);
  };

  return {
    value,
    upsert,
    isPending: update.isPending,
    refetch: query.refetch,
  };
};

/** The typed application configuration (`ChimeraAppConfig`). */
export const useSettings = () =>
  useTypedConfig<ChimeraAppConfig_Serialize, ChimeraAppConfigPatch_Serialize>(
    [CHIMERA_SETTING_QUERY_KEY],
    async () => unwrapResult(await commands.getAppConfig()),
    async (patch) => unwrapResult(await commands.patchAppConfig(patch)),
  );

/** Read or patch one typed application setting. */
type ClearableAppField = 'pac_url';

export const useSetting = <
  K extends keyof ChimeraAppConfig_Serialize &
    keyof ChimeraAppConfigPatch_Serialize,
>(
  key: K,
) =>
  useTypedConfigField<
    ChimeraAppConfig_Serialize,
    ChimeraAppConfigPatch_Serialize,
    K,
    ClearableAppField
  >(useSettings(), key);

/** Persistent typed Clash settings, separate from the running core `/configs`. */
export const useClashSettings = () =>
  useTypedConfig<ClashConfig, ClashConfigPatch_Serialize>(
    [CLASH_SETTINGS_QUERY_KEY],
    async () => unwrapResult(await commands.getClashConfig()),
    async (patch) => unwrapResult(await commands.patchClashConfig(patch)),
  );

type ClearableClashField = 'socks_port' | 'http_port';

/** Read or patch one persistent typed Clash setting. */
export const useClashSetting = <
  K extends keyof ClashConfig & keyof ClashConfigPatch_Serialize,
>(
  key: K,
) =>
  useTypedConfigField<
    ClashConfig,
    ClashConfigPatch_Serialize,
    K,
    ClearableClashField
  >(useClashSettings(), key);
