import {
  useProfileSyncLogs,
  useProfileSyncRuns,
  useProfileSyncStatus,
} from '@chimera/interface';
import { Button } from '@chimera/ui/button';
import { Card, CardContent, CardHeader } from '@chimera/ui/card';
import { useQueryClient } from '@tanstack/react-query';
import EventRepeatRounded from '~icons/material-symbols/event-repeat-rounded';
import HistoryRounded from '~icons/material-symbols/history-rounded';
import HistoryToggleOffRounded from '~icons/material-symbols/history-toggle-off-rounded';
import ScheduleRounded from '~icons/material-symbols/schedule-rounded';
import { useEffect, useRef, useState } from 'react';
import * as m from '@/paraglide/messages';
import { formatError } from '@/utils';
import { formatDate } from '@/utils/date';
import {
  Chip,
  mergeSyncRuns,
  Notice,
  Placeholder,
  RunEntry,
  RunLogs,
} from './sync-history-run';

export default function SyncHistorySection({ uid }: { uid: string }) {
  const status = useProfileSyncStatus(uid);
  const history = useProfileSyncRuns(uid);
  // undefined keeps the latest run open; an empty string is every run collapsed.
  const [expansion, setExpansion] = useState<string | undefined>(undefined);
  // Active inspection contains the current state; durable admission rows may
  // still say admitted until their owner finalizes them.
  const runs = mergeSyncRuns(
    history.data?.pages.flatMap((page) => page.items),
    status.data?.active,
  );
  const expanded = expansion ?? runs[0]?.id ?? '';
  const selected = runs.find((run) => run.id === expanded);
  const activeIds = status.data?.active.map((run) => run.id) ?? [];
  const logs = useProfileSyncLogs(
    uid,
    selected?.id ?? null,
    selected !== undefined && activeIds.includes(selected.id),
  );

  // The history and logs are not polled. A run entering or leaving the active
  // set is what changes them, so refresh both then (the logs once more to pick
  // up the lines written after the last poll).
  const queryClient = useQueryClient();
  const activeKey = status.data ? activeIds.join() : undefined;
  const previousActiveKey = useRef(activeKey);
  useEffect(() => {
    const previous = previousActiveKey.current;
    previousActiveKey.current = activeKey;
    if (previous === undefined || previous === activeKey) return;
    queryClient.invalidateQueries({ queryKey: ['profile-sync-runs', uid] });
    queryClient.invalidateQueries({ queryKey: ['profile-sync-logs', uid] });
  }, [activeKey, queryClient, uid]);
  const error = status.error ?? history.error ?? logs.error;

  return (
    <Card
      data-slot="profile-sync-history"
      data-profile-uid={uid}
      className="@container col-span-2 md:col-span-4"
    >
      <CardHeader className="flex-col items-start gap-1">
        <div className="flex items-center gap-3">
          <HistoryRounded className="text-primary size-6" aria-hidden="true" />
          <h2 className="text-lg font-medium">
            {m.profile_sync_history_title()}
          </h2>
        </div>

        {status.data && (
          <p className="text-on-surface-variant text-sm">
            {m.profile_sync_retention({ count: status.data.history_limit })}
          </p>
        )}
      </CardHeader>

      <CardContent>
        {status.data && (
          <div
            data-slot="profile-sync-summary"
            className="flex flex-wrap items-center gap-2"
          >
            <Chip
              tonal={status.data.scheduled}
              icon={
                status.data.scheduled
                  ? EventRepeatRounded
                  : HistoryToggleOffRounded
              }
            >
              {status.data.scheduled
                ? m.profile_sync_schedule_enabled()
                : m.profile_sync_schedule_disabled()}
            </Chip>

            {status.data.next_run_at && (
              <Chip icon={ScheduleRounded} className="tabular-nums">
                {m.profile_sync_next_run({
                  time: formatDate(status.data.next_run_at),
                })}
              </Chip>
            )}
          </div>
        )}

        {status.data?.journal_degraded && (
          <Notice tone="warning" role="status">
            {m.profile_sync_journal_degraded()}
          </Notice>
        )}

        {status.data?.registration_error && (
          <Notice tone="error" role="alert">
            {status.data.registration_error}
          </Notice>
        )}

        {error && (
          <Notice tone="error" role="alert">
            {formatError(error)}
          </Notice>
        )}

        {history.isPending ? (
          <Placeholder busy className="bg-surface-variant/30 rounded-3xl">
            {m.profile_sync_loading()}
          </Placeholder>
        ) : runs.length === 0 ? (
          <Placeholder
            icon={HistoryToggleOffRounded}
            className="bg-surface-variant/30 rounded-3xl"
          >
            {m.profile_sync_empty()}
          </Placeholder>
        ) : (
          <>
            <div className="divide-outline-variant divide-y">
              {runs.map((run) => (
                <RunEntry
                  key={run.id}
                  run={run}
                  expanded={run.id === expanded}
                  onToggle={() =>
                    setExpansion(run.id === expanded ? '' : run.id)
                  }
                >
                  <RunLogs logs={logs} />
                </RunEntry>
              ))}
            </div>

            {history.hasNextPage && (
              <Button
                variant="stroked"
                className="self-center"
                onClick={() => history.fetchNextPage()}
                loading={history.isFetchingNextPage}
              >
                {m.profile_sync_older()}
              </Button>
            )}
          </>
        )}
      </CardContent>
    </Card>
  );
}
