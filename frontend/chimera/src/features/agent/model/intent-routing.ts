import type {
  AgentActionRequest,
  AgentExecuteReadOnlyIntentResult,
  AgentIntent,
  AgentIntentResolution,
} from '@chimera/interface';

export type AgentIntentExecution =
  | { kind: 'diagnose' }
  | { kind: 'host_connectivity' }
  | { kind: 'proposal'; action: AgentActionRequest };

export type AgentReadOnlyIntentHandlers = {
  diagnose: () => Promise<unknown> | unknown;
  hostConnectivity: () => Promise<unknown> | unknown;
};

/** Route a closed intent without allowing read-only diagnostics to become proposals. */
export const routeAgentIntent = (intent: AgentIntent): AgentIntentExecution => {
  switch (intent.intent) {
    case 'diagnose':
      return { kind: 'diagnose' };
    case 'host_connectivity':
      return { kind: 'host_connectivity' };
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
    case 'set_service_mode':
      return {
        kind: 'proposal',
        action: { action: 'set_service_mode', enabled: intent.enabled },
      };
    case 'set_routing_mode':
      return {
        kind: 'proposal',
        action: { action: 'set_routing_mode', mode: intent.mode },
      };
    case 'start_core':
      return { kind: 'proposal', action: { action: 'start_core' } };
    case 'restart_core':
      return { kind: 'proposal', action: { action: 'restart_core' } };
    case 'reconnect_telemetry':
      return { kind: 'proposal', action: { action: 'reconnect_telemetry' } };
    case 'control_service':
      return {
        kind: 'proposal',
        action: {
          action:
            intent.operation === 'start'
              ? 'start_service'
              : intent.operation === 'stop'
                ? 'stop_service'
                : 'restart_service',
        },
      };
    case 'repair_system_proxy_endpoint':
      return {
        kind: 'proposal',
        action: { action: 'repair_system_proxy_endpoint' },
      };
    case 'disable_stale_system_proxy':
      return {
        kind: 'proposal',
        action: { action: 'disable_stale_system_proxy' },
      };
  }
};

/** Convert a backend read-only execution result into the existing closed UI resolution. */
export const resolveExecutedReadOnlyIntent = (
  result: AgentExecuteReadOnlyIntentResult,
): AgentIntentResolution => {
  switch (result.status) {
    case 'diagnosed':
      return { status: 'resolved', intent: { intent: 'diagnose' } };
    case 'host_connectivity':
      return { status: 'resolved', intent: { intent: 'host_connectivity' } };
    case 'proposal_required':
      return { status: 'resolved', intent: result.intent };
    case 'needs_clarification':
      return { status: 'needs_clarification', choices: result.choices };
    case 'unsupported':
      return { status: 'unsupported', reason: result.reason };
  }
};

/** Execute only closed read-only intents; write intents remain proposals. */
export async function executeReadOnlyAgentIntent(
  intent: AgentIntent,
  handlers: AgentReadOnlyIntentHandlers,
): Promise<boolean> {
  const execution = routeAgentIntent(intent);
  switch (execution.kind) {
    case 'diagnose':
      await handlers.diagnose();
      return true;
    case 'host_connectivity':
      await handlers.hostConnectivity();
      return true;
    case 'proposal':
      return false;
  }
}
