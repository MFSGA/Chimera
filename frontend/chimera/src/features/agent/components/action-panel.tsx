import type {
  AgentActionRequest,
  AgentNetworkSnapshot,
  AgentRoutingMode,
} from '@chimera/interface';
import {
  BuildRounded,
  RouteRounded,
  SettingsEthernetRounded,
  VpnLockRounded,
} from '@mui/icons-material';
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
  const proxyObserved = snapshot.system_proxy.observed_enabled;
  const proxyEnabledApplied =
    snapshot.system_proxy.desired_enabled &&
    proxyObserved === true &&
    snapshot.system_proxy.matches_expected_endpoint === true;
  const proxyDisabledApplied =
    !snapshot.system_proxy.desired_enabled && proxyObserved === false;
  const tunGenerated = snapshot.tun.generated_runtime_enabled;
  const tunEnabledApplied =
    snapshot.tun.desired_enabled && tunGenerated === true;
  const tunDisabledApplied =
    !snapshot.tun.desired_enabled && tunGenerated === false;

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
        <div className="mt-3 flex flex-col gap-3">
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
                  onClick={() =>
                    onPropose({ action: 'set_routing_mode', mode })
                  }
                >
                  {presentRoutingMode(mode)}
                </Button>
              ))}
            </div>
          </div>

          <div className="bg-surface-variant/25 flex flex-col gap-3 rounded-2xl p-3">
            <div className="flex items-center gap-2 font-medium">
              <SettingsEthernetRounded className="size-5" />
              {m.agent_system_proxy_title()}
            </div>
            <div className="flex flex-wrap gap-2">
              <Button
                data-slot="agent-system-proxy-enable"
                disabled={
                  pending ||
                  proxyObserved === null ||
                  snapshot.core.state !== 'running' ||
                  proxyEnabledApplied
                }
                variant="stroked"
                onClick={() =>
                  onPropose({
                    action: 'set_system_proxy_enabled',
                    enabled: true,
                  })
                }
              >
                {m.agent_enabled()}
              </Button>
              <Button
                data-slot="agent-system-proxy-disable"
                disabled={
                  pending || proxyObserved === null || proxyDisabledApplied
                }
                variant="stroked"
                onClick={() =>
                  onPropose({
                    action: 'set_system_proxy_enabled',
                    enabled: false,
                  })
                }
              >
                {m.agent_disabled()}
              </Button>
            </div>
          </div>

          <div className="bg-surface-variant/25 flex flex-col gap-3 rounded-2xl p-3">
            <div className="flex items-center gap-2 font-medium">
              <VpnLockRounded className="size-5" />
              {m.agent_tun_title()}
            </div>
            <div className="flex flex-wrap gap-2">
              <Button
                data-slot="agent-tun-enable"
                disabled={pending || tunGenerated === null || tunEnabledApplied}
                variant="stroked"
                onClick={() =>
                  onPropose({ action: 'set_tun_enabled', enabled: true })
                }
              >
                {m.agent_enabled()}
              </Button>
              <Button
                data-slot="agent-tun-disable"
                disabled={
                  pending || tunGenerated === null || tunDisabledApplied
                }
                variant="stroked"
                onClick={() =>
                  onPropose({ action: 'set_tun_enabled', enabled: false })
                }
              >
                {m.agent_disabled()}
              </Button>
            </div>
          </div>
        </div>
      </CardContent>
    </Card>
  );
}
