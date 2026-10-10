import type { AgentActionRequest, AgentIntent } from '@chimera/interface';

export type AgentIntentRoute =
  { kind: 'diagnose' } | { kind: 'proposal'; action: AgentActionRequest };

/**
 * Preserve the existing closed Agent contract. Resolving text never executes
 * a write: every non-diagnostic intent must enter the proposal flow.
 */
export function routeAgentIntent(intent: AgentIntent): AgentIntentRoute {
  switch (intent.intent) {
    case 'diagnose':
      return { kind: 'diagnose' };
    case 'set_routing_mode':
      return {
        kind: 'proposal',
        action: { action: 'set_routing_mode', mode: intent.mode },
      };
    case 'set_tun_enabled':
      return {
        kind: 'proposal',
        action: { action: 'set_tun_enabled', enabled: intent.enabled },
      };
    case 'set_system_proxy_enabled':
      return {
        kind: 'proposal',
        action: { action: 'set_system_proxy_enabled', enabled: intent.enabled },
      };
    case 'disable_stale_system_proxy':
      return {
        kind: 'proposal',
        action: { action: 'disable_stale_system_proxy' },
      };
    default: {
      const exhaustive: never = intent;
      return exhaustive;
    }
  }
}
