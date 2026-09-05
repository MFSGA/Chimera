import type { AgentActionRequest, AgentFinding } from '@chimera/interface';
import { cn } from '@chimera/ui';
import { AutoFixHighRounded, WarningAmberRounded } from '@mui/icons-material';
import { Button } from '@/components/ui/button';
import * as m from '@/paraglide/messages';
import {
  presentAgentAction,
  presentFinding,
  presentFindingSeverity,
  presentFindingTitle,
} from '../model/presenter';

const severityClass = {
  info: 'bg-secondary-container text-on-secondary-container',
  warning: 'bg-tertiary-container text-on-tertiary-container',
  critical: 'bg-error-container text-on-error-container',
} as const;

function RecommendationItem({
  finding,
  pending,
  onPropose,
}: {
  finding: AgentFinding;
  pending: boolean;
  onPropose: (action: AgentActionRequest) => void;
}) {
  const action = finding.recommended_action;

  return (
    <li className="border-outline-variant flex flex-col gap-3 rounded-2xl border p-4">
      <div className="flex items-start gap-3">
        <WarningAmberRounded className="mt-0.5 size-5 shrink-0" />
        <div className="min-w-0 flex-1">
          <div className="flex flex-wrap items-center gap-2">
            <h3 className="font-medium">{presentFindingTitle(finding.code)}</h3>
            <span
              className={cn(
                'rounded-full px-2 py-0.5 text-xs',
                severityClass[finding.severity],
              )}
            >
              {presentFindingSeverity(finding.severity)}
            </span>
          </div>
          <p className="text-on-surface-variant mt-1 text-sm">
            {presentFinding(finding.code)}
          </p>
        </div>
      </div>

      {action && (
        <div className="bg-primary-container/35 flex flex-col gap-2 rounded-2xl p-3 sm:flex-row sm:items-center sm:justify-between">
          <div>
            <p className="text-on-surface-variant text-xs font-medium">
              {m.agent_recommended_fix()}
            </p>
            <p className="text-sm font-medium">{presentAgentAction(action)}</p>
          </div>
          <Button
            className="self-start sm:self-auto"
            disabled={pending}
            variant="flat"
            onClick={() => onPropose(action)}
          >
            <AutoFixHighRounded />
            {m.agent_review_fix()}
          </Button>
        </div>
      )}
    </li>
  );
}

export function RecommendationList({
  findings,
  pending,
  onPropose,
}: {
  findings: AgentFinding[];
  pending: boolean;
  onPropose: (action: AgentActionRequest) => void;
}) {
  return (
    <ul className="flex flex-col gap-3">
      {findings.map((finding) => (
        <RecommendationItem
          finding={finding}
          key={finding.code}
          pending={pending}
          onPropose={onPropose}
        />
      ))}
    </ul>
  );
}
