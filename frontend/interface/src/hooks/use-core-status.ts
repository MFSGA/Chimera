import { useQuery } from '@tanstack/react-query';
import { commands } from '../ipc/bindings';
import { CLASH_CORE_STATUS_QUERY_KEY } from '../ipc/consts';
import { unwrapResult } from '../utils';

/** Legacy two-state IPC is a compatibility fallback once the ref Actor query
 * is available. Callers can disable its 2-second poll without losing cached
 * values, or explicitly refetch it for diagnostics. */
export function useCoreStatus({ enabled = true }: { enabled?: boolean } = {}) {
  return useQuery({
    enabled,
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
