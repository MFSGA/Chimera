import type {
  AgentAuditHistoryEntry,
  AgentDiagnosticHistoryEntry,
  AgentHistorySnapshot,
  AgentHistorySummary,
} from '@chimera/interface';
import {
  DeleteSweepRounded,
  HistoryRounded,
  RefreshRounded,
} from '@mui/icons-material';
import type { ReactNode } from 'react';
import { Button } from '@/components/ui/button';
import { Card, CardContent, CardHeader } from '@/components/ui/card';
import * as m from '@/paraglide/messages';
import {
  presentActionKind,
  presentAuditOutcome,
  presentCoreState,
  presentFinding,
  presentHealth,
  presentProbe,
  presentServiceState,
} from '../model/presenter';

const MAX_VISIBLE_ENTRIES = 20;

export function HistoryCard({
  history,
  loading,
  clearing,
  onRefresh,
  onClear,
}: {
  history: AgentHistorySnapshot | undefined;
  loading: boolean;
  clearing: boolean;
  onRefresh: () => void;
  onClear: () => void;
}) {
  const diagnostics =
    history?.diagnostics.slice(-MAX_VISIBLE_ENTRIES).reverse() ?? [];
  const audits = history?.audits.slice(-MAX_VISIBLE_ENTRIES).reverse() ?? [];
  const hasHistory = diagnostics.length > 0 || audits.length > 0;

  return (
    <Card variant="outline">
      <CardHeader className="text-base">
        <HistoryRounded />
        {m.agent_history_title()}
        <div className="ml-auto flex flex-wrap gap-2">
          <Button loading={loading} variant="flat" onClick={onRefresh}>
            <RefreshRounded />
            {m.agent_history_refresh()}
          </Button>
          <Button
            disabled={!hasHistory}
            loading={clearing}
            variant="flat"
            onClick={onClear}
          >
            <DeleteSweepRounded />
            {m.agent_history_clear()}
          </Button>
        </div>
      </CardHeader>
      <CardContent>
        <p className="text-on-surface-variant text-sm">
          {m.agent_history_description()}
        </p>
        {history && <HistorySummary summary={history.summary} />}
        <div className="grid gap-4 xl:grid-cols-2">
          <HistorySection
            title={m.agent_diagnostic_history()}
            empty={diagnostics.length === 0}
          >
            {diagnostics.map((entry) => (
              <DiagnosticEntry
                key={`${entry.captured_at}-${entry.revision}`}
                entry={entry}
              />
            ))}
          </HistorySection>
          <HistorySection
            title={m.agent_audit_history()}
            empty={audits.length === 0}
          >
            {audits.map((entry) => (
              <AuditEntry
                key={`${entry.recorded_at}-${entry.proposal_id}-${entry.outcome}`}
                entry={entry}
              />
            ))}
          </HistorySection>
        </div>
      </CardContent>
    </Card>
  );
}

function HistorySummary({ summary }: { summary: AgentHistorySummary }) {
  const recurringIssues = [
    ...summary.finding_counts.map((item) => ({
      key: `finding:${item.code}`,
      label: presentFinding(item.code),
      count: item.count,
    })),
    ...summary.probe_failure_counts.map((item) => ({
      key: `probe:${item.code}`,
      label: presentProbe(item.code),
      count: item.count,
    })),
  ]
    .sort((left, right) => right.count - left.count)
    .slice(0, 3);

  return (
    <section className="bg-surface-variant/20 flex flex-col gap-3 rounded-2xl p-3">
      <h3 className="font-medium">{m.agent_history_summary()}</h3>
      <div className="grid gap-2 sm:grid-cols-2 xl:grid-cols-4">
        <SummaryMetric
          label={m.agent_history_latest_health()}
          value={
            summary.latest_health
              ? presentHealth(summary.latest_health)
              : m.agent_unknown()
          }
        />
        <SummaryMetric
          label={m.agent_history_trend()}
          value={presentHealthTrend(summary.health_trend)}
        />
        <SummaryMetric
          label={m.agent_history_unhealthy_ratio()}
          value={`${summary.unhealthy_samples}/${summary.diagnostic_samples}`}
        />
        <SummaryMetric
          label={m.agent_history_verified_ratio()}
          value={`${summary.verified_actions}/${summary.action_attempts}`}
        />
      </div>
      <div>
        <p className="text-sm font-medium">
          {m.agent_history_recurring_issues()}
        </p>
        {recurringIssues.length === 0 ? (
          <p className="text-on-surface-variant mt-1 text-sm">
            {m.agent_history_no_recurring_issues()}
          </p>
        ) : (
          <div className="mt-2 flex flex-wrap gap-2">
            {recurringIssues.map((item) => (
              <span
                className="border-outline-variant rounded-full border px-2 py-1 text-xs"
                key={item.key}
              >
                {item.label} · {item.count}
              </span>
            ))}
          </div>
        )}
      </div>
    </section>
  );
}

function SummaryMetric({ label, value }: { label: string; value: string }) {
  return (
    <div className="bg-surface rounded-xl p-3">
      <p className="text-on-surface-variant text-xs">{label}</p>
      <p className="mt-1 font-medium">{value}</p>
    </div>
  );
}

function presentHealthTrend(trend: AgentHistorySummary['health_trend']) {
  if (trend === 'improving') return m.agent_history_trend_improving();
  if (trend === 'worsening') return m.agent_history_trend_worsening();
  if (trend === 'stable') return m.agent_history_trend_stable();
  return m.agent_history_trend_insufficient_data();
}

function HistorySection({
  title,
  empty,
  children,
}: {
  title: string;
  empty: boolean;
  children: ReactNode;
}) {
  return (
    <section className="flex min-w-0 flex-col gap-2">
      <h3 className="font-medium">{title}</h3>
      {empty ? (
        <p className="bg-surface-variant/25 text-on-surface-variant rounded-2xl p-3 text-sm">
          {m.agent_history_empty()}
        </p>
      ) : (
        <div className="max-h-96 space-y-2 overflow-y-auto pr-1">
          {children}
        </div>
      )}
    </section>
  );
}

function DiagnosticEntry({ entry }: { entry: AgentDiagnosticHistoryEntry }) {
  return (
    <article className="bg-surface-variant/25 rounded-2xl p-3 text-sm">
      <div className="flex flex-wrap items-center justify-between gap-2">
        <strong>{presentHealth(entry.health)}</strong>
        <time className="text-on-surface-variant text-xs">
          {new Date(entry.captured_at).toLocaleString()}
        </time>
      </div>
      <p className="text-on-surface-variant mt-1">
        {presentCoreState(entry.core_state)} ·{' '}
        {presentServiceState(entry.service_state)}
      </p>
      <p className="text-on-surface-variant mt-1 text-xs">
        {m.agent_history_findings({ count: entry.finding_codes.length })} ·{' '}
        {m.agent_history_probe_failures({
          count: entry.probe_failure_codes.length,
        })}
      </p>
    </article>
  );
}

function AuditEntry({ entry }: { entry: AgentAuditHistoryEntry }) {
  return (
    <article className="bg-surface-variant/25 rounded-2xl p-3 text-sm">
      <div className="flex flex-wrap items-center justify-between gap-2">
        <strong>{presentActionKind(entry.action)}</strong>
        <time className="text-on-surface-variant text-xs">
          {new Date(entry.recorded_at).toLocaleString()}
        </time>
      </div>
      <p className="text-on-surface-variant mt-1">
        {presentAuditOutcome(entry.outcome)}
      </p>
      <p className="text-on-surface-variant mt-1 truncate text-xs">
        {m.agent_history_proposal()}: {entry.proposal_id}
      </p>
    </article>
  );
}
