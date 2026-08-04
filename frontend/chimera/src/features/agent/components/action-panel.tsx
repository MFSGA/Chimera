import type {
  AgentActionRequest,
  AgentNetworkSnapshot,
  AgentRecommendation,
} from '@chimera/interface';
import { BuildRounded } from '@mui/icons-material';
import { Button } from '@/components/ui/button';
import { Card, CardContent, CardHeader } from '@/components/ui/card';
import * as m from '@/paraglide/messages';
import { presentRoutingMode } from '../model/presenter';

const presentAction = (recommendation: AgentRecommendation) => {
  const action = recommendation.action;
  switch (action.action) {
    case 'set_routing_mode':
      return `${m.agent_set_mode()}: ${presentRoutingMode(action.mode)}`;
    case 'set_tun_enabled':
      return `${m.agent_tun_title()}: ${action.enabled ? m.agent_enabled() : m.agent_disabled()}`;
    case 'set_system_proxy_enabled':
      return `${m.agent_system_proxy_title()}: ${action.enabled ? m.agent_enabled() : m.agent_disabled()}`;
    case 'set_service_mode':
      return `${m.settings_system_proxy_service_mode_label()}: ${action.enabled ? m.agent_enabled() : m.agent_disabled()}`;
    case 'start_core':
      return m.agent_start_core();
    case 'restart_core':
      return m.agent_restart_core();
    case 'reconnect_telemetry':
      return m.agent_reconnect_telemetry();
    case 'start_service':
      return m.agent_start_service();
    case 'stop_service':
      return m.agent_stop_service();
    case 'restart_service':
      return m.agent_restart_service();
    case 'repair_system_proxy_endpoint':
      return m.agent_repair_proxy_endpoint();
    case 'disable_stale_system_proxy':
      return m.agent_disable_stale_proxy();
    default: {
      const exhaustive: never = action;
      return exhaustive;
    }
  }
};

export function ActionPanel({
  snapshot,
  pending,
  onPropose,
}: {
  snapshot: AgentNetworkSnapshot;
  pending: boolean;
  onPropose: (action: AgentActionRequest) => void;
}) {
  const available = snapshot.recommendations.filter(
    (recommendation) => recommendation.available,
  );

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
        <div className="flex flex-wrap gap-2">
          {available.map((recommendation) => (
            <Button
              disabled={pending}
              key={JSON.stringify(recommendation.action)}
              variant="stroked"
              onClick={() => onPropose(recommendation.action)}
            >
              {presentAction(recommendation)}
            </Button>
          ))}
        </div>
      </CardContent>
    </Card>
  );
}
