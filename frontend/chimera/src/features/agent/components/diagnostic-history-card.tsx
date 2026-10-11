import type { AgentDiagnosticHistoryView } from '@chimera/interface';
import {
  HistoryRounded,
  RefreshRounded,
  SaveRounded,
} from '@mui/icons-material';
import { Button } from '@/components/ui/button';
import { Card, CardContent, CardHeader } from '@/components/ui/card';
import * as m from '@/paraglide/messages';

const displayCode = (code: string) => code.replaceAll('_', ' ');

/** Read-only projection; only an explicit Save action captures new state. */
export function DiagnosticHistoryCard({
  history,
  capturing,
  loading,
  error,
  onCapture,
  onRefresh,
}: {
  history: AgentDiagnosticHistoryView | undefined;
  capturing: boolean;
  loading: boolean;
  error: boolean;
  onCapture: () => void;
  onRefresh: () => void;
}) {
  return (
    <Card variant="outline" data-slot="agent-diagnostic-history">
      <CardHeader className="flex-wrap gap-2 text-base">
        <HistoryRounded />
        {m.agent_diagnostic_history()}
        <div className="ml-auto flex flex-wrap gap-2">
          <Button
            disabled={loading || capturing}
            loading={capturing}
            variant="flat"
            onClick={onCapture}
          >
            <SaveRounded />
            {m.agent_diagnostic_capture()}
          </Button>
          <Button
            disabled={capturing}
            loading={loading}
            variant="flat"
            onClick={onRefresh}
          >
            <RefreshRounded />
            {m.agent_history_refresh()}
          </Button>
        </div>
      </CardHeader>
      <CardContent>
        <p className="text-on-surface-variant text-sm">
          {m.agent_diagnostic_history_description()}
        </p>
        {error && <p className="text-error text-sm">{m.agent_error_title()}</p>}
        {history && (
          <>
            <p className="text-on-surface-variant text-sm">
              {m.agent_audit_retained_events({
                count: history.retained_events,
              })}
            </p>
            {history.entries.length === 0 ? (
              <p className="text-on-surface-variant text-sm">
                {m.agent_history_empty()}
              </p>
            ) : (
              <ol className="divide-outline-variant divide-y text-sm">
                {history.entries.map((entry, index) => (
                  <li key={`${entry.captured_at}-${index}`} className="py-2">
                    <div className="flex flex-wrap items-center justify-between gap-2">
                      <span className="font-medium">
                        {displayCode(entry.health)} ·{' '}
                        {displayCode(entry.core_state)}
                        {' · '}
                        {displayCode(entry.service_state)}
                      </span>
                      <time className="text-on-surface-variant text-xs">
                        {new Date(entry.captured_at).toLocaleString()}
                      </time>
                    </div>
                    {(entry.finding_codes.length > 0 ||
                      entry.probe_failure_codes.length > 0) && (
                      <p className="text-on-surface-variant mt-1 text-xs break-words">
                        {[...entry.finding_codes, ...entry.probe_failure_codes]
                          .map(displayCode)
                          .join(' · ')}
                      </p>
                    )}
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
