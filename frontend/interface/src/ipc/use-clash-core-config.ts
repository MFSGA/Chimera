import { useMutation, useQueryClient } from '@tanstack/react-query';
import { unwrapResult } from '../utils';
import { commands, PatchClashCoreConfig } from './bindings';
import { CLASH_CONFIG_QUERY_KEY, CLASH_INFO_QUERY_KEY } from './consts';

export const useClashCoreConfig = () => {
  const queryClient = useQueryClient();

  const upsert = useMutation({
    mutationFn: async (payload: PatchClashCoreConfig) => {
      return unwrapResult(await commands.patchClashCoreConfig(payload));
    },
    onSuccess: () => {
      queryClient.invalidateQueries({ queryKey: [CLASH_CONFIG_QUERY_KEY] });
      queryClient.invalidateQueries({ queryKey: [CLASH_INFO_QUERY_KEY] });
    },
  });

  return {
    upsert,
  };
};
