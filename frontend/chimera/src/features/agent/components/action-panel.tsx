import type {
  AgentActionRequest,
  AgentNetworkSnapshot,
  AgentRoutingMode,
} from '@chimera/interface';
import { BuildRounded, RouteRounded } from '@mui/icons-material';
import { Button } from '@/components/ui/button';
import { Card, CardContent, CardHeader } from '@/components/ui/card';
import * as m from '@/paraglide/messages';
import { presentRoutingMode } from '../model/presenter';

const modes: AgentRoutingMode[] = ['rule', 'global', 'direct'];

export function ActionPanel({
  snapshot,
  pending,
  onPropose,
}: {
  snapshot: AgentNetworkSnapshot;
  pending: boolean;
  onPropose: (action: AgentActionRequest) => void;
}) {
  const staleProxyCanBeDisabled =
    snapshot.core.state === 'stopped' &&
    snapshot.system_proxy.observed_enabled === true &&
    snapshot.system_proxy.matches_expected_endpoint === true;

  return (
    <Card variant="outline">
      <CardHeader className="text-base">
        <BuildRounded />
        {m.agent_actions_title()}
      </CardHeader>
      <CardContent>
        <p className="text-on-surface-variant text-sm">
          {m.agent_actions_description()}
        </p>
        <div className="bg-surface-variant/25 flex flex-col gap-3 rounded-2xl p-3">
          <div className="flex items-center gap-2 font-medium">
            <RouteRounded className="size-5" />
            {m.agent_set_mode()}
          </div>
          <div className="flex flex-wrap gap-2">
            {modes.map((mode) => (
              <Button
                disabled={
                  pending ||
                  snapshot.core.state !== 'running' ||
                  snapshot.core.routing_mode === null ||
                  snapshot.core.observed_routing_mode !==
                    snapshot.core.routing_mode ||
                  snapshot.core.routing_mode === mode
                }
                key={mode}
                variant="stroked"
                onClick={() => onPropose({ action: 'set_routing_mode', mode })}
              >
                {presentRoutingMode(mode)}
              </Button>
            ))}
          </div>
        </div>
        {staleProxyCanBeDisabled && (
          <Button
            disabled={pending}
            variant="stroked"
            onClick={() => onPropose({ action: 'disable_stale_system_proxy' })}
          >
            {m.agent_disable_stale_proxy()}
          </Button>
        )}
      </CardContent>
    </Card>
  );
}
