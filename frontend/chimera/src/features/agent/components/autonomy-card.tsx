import type { AgentAutonomyPolicySnapshot } from '@chimera/interface';
import { Button } from '@/components/ui/button';
import { Card, CardContent, CardHeader } from '@/components/ui/card';
import * as m from '@/paraglide/messages';
import { presentAutonomyStatus } from '../model/autonomy-status';

export function AutonomyCard({
  policy,
  authorizing,
  revoking,
  onAuthorize,
  onRevoke,
}: {
  policy: AgentAutonomyPolicySnapshot | undefined;
  authorizing: boolean;
  revoking: boolean;
  onAuthorize: () => void;
  onRevoke: () => void;
}) {
  const status = presentAutonomyStatus(policy?.status);
  const active = policy?.enabled === true && status === 'active';
  const statusMessage = active
    ? m.agent_autonomy_active({
        remaining: policy.remaining_actions,
        expires: new Date(policy.expires_at * 1000).toLocaleTimeString(),
      })
    : status === 'expired'
      ? m.agent_autonomy_expired()
      : status === 'revoked'
        ? m.agent_autonomy_revoked()
        : status === 'session_mismatch'
          ? m.agent_autonomy_session_mismatch()
          : m.agent_autonomy_inactive();
  return (
    <Card variant="raised">
      <CardHeader className="text-base">{m.agent_autonomy_title()}</CardHeader>
      <CardContent>
        <p className="text-on-surface-variant text-sm">
          {m.agent_autonomy_description()}
        </p>
        <p className="mt-2 text-sm font-medium">{statusMessage}</p>
        <div className="mt-3 flex flex-wrap gap-2">
          <Button
            disabled={active || revoking}
            loading={authorizing}
            onClick={onAuthorize}
          >
            {m.agent_autonomy_authorize()}
          </Button>
          <Button
            disabled={!active || authorizing}
            loading={revoking}
            variant="stroked"
            onClick={onRevoke}
          >
            {m.agent_autonomy_revoke()}
          </Button>
        </div>
        <p className="text-on-surface-variant mt-2 text-xs">
          {m.agent_autonomy_scope_notice()}
        </p>
      </CardContent>
    </Card>
  );
}
