import type { AgentFinding, AgentProbeFailure } from '@chimera/interface';
import { cn } from '@chimera/ui';
import { CheckCircleRounded, WarningAmberRounded } from '@mui/icons-material';
import { Card, CardContent, CardHeader } from '@/components/ui/card';
import * as m from '@/paraglide/messages';
import { presentFinding, presentProbe } from '../model/presenter';

const severityClass = {
  info: 'bg-secondary-container text-on-secondary-container',
  warning: 'bg-tertiary-container text-on-tertiary-container',
  critical: 'bg-error-container text-on-error-container',
} as const;

function FindingRow({ finding }: { finding: AgentFinding }) {
  return (
    <li className="bg-surface-variant/30 flex items-start gap-3 rounded-2xl p-3">
      <WarningAmberRounded className="mt-0.5 size-5 shrink-0" />
      <span className="min-w-0 flex-1">{presentFinding(finding.code)}</span>
      <span
        className={cn(
          'rounded-full px-2 py-0.5 text-xs',
          severityClass[finding.severity],
        )}
      >
        {finding.severity}
      </span>
    </li>
  );
}

export function FindingList({ findings }: { findings: AgentFinding[] }) {
  return (
    <Card variant="outline">
      <CardHeader className="text-base">{m.agent_findings_title()}</CardHeader>
      <CardContent>
        {findings.length === 0 ? (
          <div className="flex items-center gap-2 text-sm">
            <CheckCircleRounded className="text-primary size-5" />
            {m.agent_no_findings()}
          </div>
        ) : (
          <ul className="flex flex-col gap-2">
            {findings.map((finding) => (
              <FindingRow finding={finding} key={finding.code} />
            ))}
          </ul>
        )}
      </CardContent>
    </Card>
  );
}

export function ProbeFailureList({
  failures,
}: {
  failures: AgentProbeFailure[];
}) {
  if (failures.length === 0) return null;
  return (
    <Card variant="outline">
      <CardHeader className="text-base">
        {m.agent_probe_failures_title()}
      </CardHeader>
      <CardContent asChild>
        <ul className="list-disc gap-2 pl-9 text-sm">
          {failures.map((failure) => (
            <li key={failure.code}>{presentProbe(failure.code)}</li>
          ))}
        </ul>
      </CardContent>
    </Card>
  );
}
