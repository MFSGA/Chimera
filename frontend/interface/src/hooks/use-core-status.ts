import { useQuery } from '@tanstack/react-query';
import { commands } from '../ipc/bindings';
import { CLASH_CORE_STATUS_QUERY_KEY } from '../ipc/consts';
import { unwrapResult } from '../utils';

/** Poll the desktop core runtime state through the typed IPC contract. */
export function useCoreStatus() {
  return useQuery({
    queryKey: [CLASH_CORE_STATUS_QUERY_KEY],
    queryFn: async () => {
      const result = unwrapResult(await commands.getCoreStatus());
      if (!result) return null;

      const [status, startAt, type] = result;
      return { status, startAt, type };
    },
    refetchInterval: 2000,
    refetchOnWindowFocus: false,
  });
}
