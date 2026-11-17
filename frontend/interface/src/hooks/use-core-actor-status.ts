import { useQuery, useQueryClient } from '@tanstack/react-query';
import { useEffect } from 'react';
import { commands, events } from '../ipc/bindings';
import { unwrapResult } from '../utils';

/** Ref-style routing state: unlike the legacy two-state core query this
 * distinguishes handoff, degradation, stopping and unknown snapshots.
 *
 * Query first to cover the period before an event listener is installed.
 * Events invalidate the snapshot; infrequent polling is only recovery for
 * missed events (suspended windows or transient listener failures).
 */
export const CORE_ACTOR_STATUS_QUERY_KEY = 'getCoreActorStatus';

export function useCoreActorStatus() {
  const queryClient = useQueryClient();

  useEffect(() => {
    let active = true;
    const listener = events.coreStatusChangedEvent.listen(() => {
      if (active) {
        void queryClient.invalidateQueries({
          queryKey: [CORE_ACTOR_STATUS_QUERY_KEY],
        });
      }
    });
    return () => {
      active = false;
      void listener.then((unlisten) => unlisten());
    };
  }, [queryClient]);

  return useQuery({
    queryKey: [CORE_ACTOR_STATUS_QUERY_KEY],
    queryFn: async () => unwrapResult(await commands.getCoreActorStatus()),
    refetchInterval: 15_000,
    refetchOnWindowFocus: true,
  });
}
