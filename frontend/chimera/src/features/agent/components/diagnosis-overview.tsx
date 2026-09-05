import type {
  AgentActionRequest,
  AgentNetworkSnapshot,
} from '@chimera/interface';
import {
  CheckCircleRounded,
  HealthAndSafetyRounded,
  WarningAmberRounded,
} from '@mui/icons-material';
import { Card, CardContent, CardHeader } from '@/components/ui/card';
import * as m from '@/paraglide/messages';
import { presentHealth } from '../model/presenter';
import { RecommendationList } from './recommendation-list';

function ResultMessage({ snapshot }: { snapshot: AgentNetworkSnapshot }) {
  if (snapshot.findings.length > 0) {
    return (
      <p className="text-on-surface-variant text-sm">
        {m.agent_result_summary({ count: snapshot.findings.length })}
      </p>
    );
  }

  if (snapshot.probe_failures.length > 0) {
    return (
      <div className="flex items-start gap-2 text-sm">
        <WarningAmberRounded className="mt-0.5 size-5 shrink-0" />
        <span>{m.agent_result_partial()}</span>
      </div>
    );
  }

  return (
    <div className="flex items-start gap-3">
      <CheckCircleRounded className="text-primary mt-0.5 size-6 shrink-0" />
      <div>
        <p className="font-medium">{m.agent_all_clear_title()}</p>
        <p className="text-on-surface-variant mt-1 text-sm">
          {m.agent_all_clear_description()}
        </p>
      </div>
    </div>
  );
}

export function DiagnosisOverview({
  snapshot,
  pending,
  onPropose,
}: {
  snapshot: AgentNetworkSnapshot;
  pending: boolean;
  onPropose: (action: AgentActionRequest) => void;
}) {
  return (
    <Card variant="raised">
      <CardHeader className="text-base">
        <HealthAndSafetyRounded />
        {m.agent_result_title()}
      </CardHeader>
      <CardContent>
        <div className="flex flex-wrap items-center justify-between gap-2">
          <div>
            <p className="text-on-surface-variant text-xs">
              {m.agent_health_title()}
            </p>
            <p className="text-lg font-semibold">
              {presentHealth(snapshot.health)}
            </p>
          </div>
          <p className="text-on-surface-variant text-xs">
            {m.agent_captured_at()}:{' '}
            {new Date(snapshot.captured_at).toLocaleString()}
          </p>
        </div>

        <ResultMessage snapshot={snapshot} />

        {snapshot.findings.length > 0 && (
          <RecommendationList
            findings={snapshot.findings}
            pending={pending}
            onPropose={onPropose}
          />
        )}
      </CardContent>
    </Card>
  );
}
