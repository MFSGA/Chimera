import { useMutation, useQuery, useQueryClient } from '@tanstack/react-query';
import { mutations, queries } from '../ipc/hotkey-bindings.js';
import { invokeMutation, invokeQuery } from '../ipc/query-options.js';
import { unwrapResult } from '../utils/index.js';

export function useHotkeys() {
  const queryClient = useQueryClient();
  const hotkeysQuery = queries.getHotkeys();
  const setHotkeys = mutations.setHotkeys;

  const query = useQuery({
    queryKey: hotkeysQuery.queryKey,
    queryFn: async () => unwrapResult(await invokeQuery(hotkeysQuery)),
  });

  const update = useMutation({
    mutationKey: setHotkeys.mutationKey,
    // Keep the MutationOutcome intact so the shared MutationCache can surface
    // OS registration failures after the config has committed.
    mutationFn: async (hotkeys: string[]) =>
      unwrapResult(await invokeMutation(setHotkeys, [hotkeys])),
    onSuccess: () => {
      queryClient.invalidateQueries({
        queryKey: queries.getHotkeys().queryKey,
      });
    },
  });

  return {
    ...query,
    data: query.data ?? [],
    mutate: update.mutateAsync,
  };
}
