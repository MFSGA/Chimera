import type { AgentAuditHistoryView } from '@chimera/interface';
import { Button } from '@chimera/ui/button';
import { Card, CardContent, CardHeader } from '@chimera/ui/card';
import { HistoryRounded, RefreshRounded } from '@mui/icons-material';
import * as m from '@/paraglide/messages';

const displayCode = (code: string) => code.replaceAll('_', ' ');

/**
 * Read-only projection of the existing bounded audit store.
 * No proposal IDs, revisions, raw logs, targets, or profile data are exposed.
 */
export function AuditHistoryCard({
  history,
  loading,
  error,
  onRefresh,
}: {
  history: AgentAuditHistoryView | undefined;
  loading: boolean;
  error: boolean;
  onRefresh: () => void;
}) {
  return (
    <Card variant="outline" data-slot="agent-audit-history">
      <CardHeader className="text-base">
        <HistoryRounded />
        {m.agent_audit_history()}
        <Button
          className="ml-auto"
          loading={loading}
          variant="flat"
          data-slot="agent-audit-history-refresh"
          onClick={onRefresh}
        >
          <RefreshRounded />
          {m.agent_history_refresh()}
        </Button>
      </CardHeader>
      <CardContent>
        <p className="text-on-surface-variant text-sm">
          {m.agent_audit_history_description()}
        </p>
        {error && <p className="text-error text-sm">{m.agent_error_title()}</p>}
        {history && (
          <>
            <p className="text-on-surface-variant text-sm">
              {m.agent_audit_retained_events({
                count: history.retained_events,
              })}
              {' · '}
              {m.agent_history_verified_ratio()}: {history.verified_events}
            </p>
            {history.entries.length === 0 ? (
              <p className="text-on-surface-variant text-sm">
                {m.agent_history_empty()}
              </p>
            ) : (
              <ol className="divide-outline-variant divide-y text-sm">
                {history.entries.map((entry, index) => (
                  <li
                    className="flex flex-wrap items-center justify-between gap-2 py-2"
                    key={`${entry.recorded_at}-${index}`}
                  >
                    <div className="flex flex-wrap gap-2">
                      <span className="font-medium">
                        {displayCode(entry.action)}
                      </span>
                      <code className="text-on-surface-variant">
                        {displayCode(entry.outcome)}
                      </code>
                    </div>
                    <time className="text-on-surface-variant text-xs">
                      {new Date(entry.recorded_at).toLocaleString()}
                    </time>
                  </li>
                ))}
              </ol>
            )}
          </>
        )}
      </CardContent>
    </Card>
  );
}
