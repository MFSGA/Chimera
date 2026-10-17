import {
  useInfiniteQuery,
  useIsMutating,
  useQuery,
  useQueryClient,
} from '@tanstack/react-query';
import { useEffect, useRef } from 'react';
import { unwrapResult } from '../utils';
import {
  commands,
  type ProfileSyncStatus,
  type RunCursorDto,
} from './bindings';
import { mutations } from './profile-bindings';

const LIVE_INTERVAL = 2_000;
const IDLE_INTERVAL = 60_000;

const statusInterval = (
  status: ProfileSyncStatus | undefined,
  refreshing: boolean,
) => {
  if (refreshing || (status?.active.length ?? 0) > 0) return LIVE_INTERVAL;
  if (!status?.next_run_at) return IDLE_INTERVAL;
  const untilNextRun = Date.parse(status.next_run_at) - Date.now() + 1_000;
  return Math.min(Math.max(untilNextRun, LIVE_INTERVAL), IDLE_INTERVAL);
};

export function didScheduledRunAdvance(
  previousNextRunAt: string | null,
  nextRunAt: string | null,
  now = Date.now(),
) {
  if (!previousNextRunAt || previousNextRunAt === nextRunAt) return false;
  const previousTime = Date.parse(previousNextRunAt);
  return Number.isFinite(previousTime) && previousTime <= now;
}

export function didHistoryRevisionAdvance(
  previous: string | null | undefined,
  next: string | null,
) {
  if (previous === undefined) return next !== null;
  return previous !== next;
}

/** Poll quickly only while a run is active or this Profile is updating. */
export function useProfileSyncStatus(uid: string) {
  const refreshing =
    useIsMutating({
      mutationKey: mutations.updateProfile.mutationKey,
      predicate: (mutation) =>
        (mutation.state.variables as { uid?: string } | undefined)?.uid === uid,
    }) > 0;
  const queryClient = useQueryClient();
  const wasRefreshing = useRef(false);
  const previousNextRunAt = useRef<string | null | undefined>(undefined);
  const previousHistoryRevision = useRef<string | null | undefined>(undefined);
  const status = useQuery({
    queryKey: ['profile-sync-status', uid],
    queryFn: async () => unwrapResult(await commands.getProfileSyncStatus(uid)),
    refetchInterval: (query) => statusInterval(query.state.data, refreshing),
  });

  useEffect(() => {
    if (wasRefreshing.current && !refreshing) {
      // A fast loopback/manual sync can finish between status polls. Refresh
      // the durable pages when the shared update mutation settles as well.
      void queryClient.invalidateQueries({
        queryKey: ['profile-sync-runs', uid],
      });
      void queryClient.invalidateQueries({
        queryKey: ['profile-sync-logs', uid],
      });
      void queryClient.invalidateQueries({
        queryKey: ['profile-sync-status', uid],
      });
    }
    wasRefreshing.current = refreshing;
  }, [queryClient, refreshing, uid]);

  const nextRunAt = status.data?.next_run_at ?? null;
  const historyRevision = status.data?.latest_run_sequence ?? null;
  useEffect(() => {
    const previous = previousHistoryRevision.current;
    previousHistoryRevision.current = historyRevision;
    if (!didHistoryRevisionAdvance(previous, historyRevision)) {
      return;
    }
    // Another window or the agent can finish a manual run while this WebView
    // is idle. The durable admission sequence exposes that change on polling.
    void queryClient.invalidateQueries({
      queryKey: ['profile-sync-runs', uid],
    });
    void queryClient.invalidateQueries({
      queryKey: ['profile-sync-logs', uid],
    });
  }, [historyRevision, queryClient, uid]);

  useEffect(() => {
    const previous = previousNextRunAt.current;
    previousNextRunAt.current = nextRunAt;
    if (
      previous === undefined ||
      !didScheduledRunAdvance(previous, nextRunAt)
    ) {
      return;
    }
    // A scheduled run can start and finish between status polls. Its next
    // deadline moving forward is the durable signal that history changed.
    void queryClient.invalidateQueries({
      queryKey: ['profile-sync-runs', uid],
    });
    void queryClient.invalidateQueries({
      queryKey: ['profile-sync-logs', uid],
    });
  }, [nextRunAt, queryClient, uid]);

  return status;
}

/** Run history is stable except when a run enters or leaves the active set. */
export function useProfileSyncRuns(uid: string) {
  return useInfiniteQuery({
    queryKey: ['profile-sync-runs', uid],
    initialPageParam: null as RunCursorDto | null,
    queryFn: async ({ pageParam }) =>
      unwrapResult(await commands.getProfileSyncRuns(uid, pageParam)),
    getNextPageParam: (page) => page.next ?? undefined,
  });
}

/** Finished-run logs are immutable; only live logs need frequent polling. */
export function useProfileSyncLogs(
  uid: string,
  run: string | null,
  live: boolean,
) {
  return useInfiniteQuery({
    queryKey: ['profile-sync-logs', uid, run],
    initialPageParam: null as string | null,
    queryFn: async ({ pageParam }) =>
      unwrapResult(await commands.getProfileSyncLogs(uid, run!, pageParam)),
    getNextPageParam: (page) => page.next ?? undefined,
    enabled: run !== null,
    refetchInterval: live ? LIVE_INTERVAL : false,
  });
}
