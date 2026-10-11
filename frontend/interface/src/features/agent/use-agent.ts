import { useMutation, useQuery, useQueryClient } from '@tanstack/react-query';
import {
  commands,
  type AgentActionRequest,
  type AgentAuditHistoryView,
  type AgentDiagnosticHistoryView,
  type AgentNetworkProbeRequest,
  type AgentNetworkSnapshot,
  type AgentToolRequest,
} from '../../ipc/bindings';
import { unwrapResult } from '../../utils';

export const AGENT_MANIFEST_QUERY_KEY = ['agent', 'manifest'] as const;

export const AGENT_NETWORK_SNAPSHOT_QUERY_KEY = [
  'agent',
  'network-snapshot',
] as const;

export const AGENT_AUDIT_HISTORY_QUERY_KEY = [
  'agent',
  'audit-history',
] as const;

export const AGENT_DIAGNOSTIC_HISTORY_QUERY_KEY = [
  'agent',
  'diagnostic-history',
] as const;

export type AgentExecuteInput = {
  proposalId: string;
  digest: string;
};

const getSystemSnapshot = async (): Promise<AgentNetworkSnapshot> => {
  const result = unwrapResult(
    await commands.agentExecuteTool({ tool: 'system.snapshot' }),
  );
  if (result.tool !== 'system.snapshot') {
    throw new Error('agent_tool_result_mismatch');
  }
  return result.output;
};

/**
 * Provides the explicit-read and proposal lifecycle for the network agent.
 * The snapshot never polls or loads automatically; callers trigger `refetch`.
 */
export const useAgent = () => {
  const queryClient = useQueryClient();

  const manifest = useQuery({
    queryKey: AGENT_MANIFEST_QUERY_KEY,
    queryFn: commands.agentGetManifest,
    staleTime: Infinity,
    retry: false,
  });

  const snapshot = useQuery({
    queryKey: AGENT_NETWORK_SNAPSHOT_QUERY_KEY,
    queryFn: getSystemSnapshot,
    enabled: false,
    retry: false,
    refetchInterval: false,
    refetchOnMount: false,
    refetchOnReconnect: false,
    refetchOnWindowFocus: false,
  });

  // Audit history is only read on explicit user request; never background-poll.
  const auditHistory = useQuery<AgentAuditHistoryView>({
    queryKey: AGENT_AUDIT_HISTORY_QUERY_KEY,
    queryFn: async () => unwrapResult(await commands.agentGetAuditHistory()),
    enabled: false,
    retry: false,
    refetchInterval: false,
    refetchOnMount: false,
    refetchOnReconnect: false,
    refetchOnWindowFocus: false,
  });

  const diagnosticHistory = useQuery<AgentDiagnosticHistoryView>({
    queryKey: AGENT_DIAGNOSTIC_HISTORY_QUERY_KEY,
    queryFn: async () =>
      unwrapResult(await commands.agentGetDiagnosticHistory()),
    enabled: false,
    retry: false,
    refetchInterval: false,
    refetchOnMount: false,
    refetchOnReconnect: false,
    refetchOnWindowFocus: false,
  });

  const captureDiagnostic = useMutation({
    mutationFn: async () =>
      unwrapResult(await commands.agentCaptureDiagnosticHistory()),
    onSuccess: (history) => {
      queryClient.setQueryData(AGENT_DIAGNOSTIC_HISTORY_QUERY_KEY, history);
    },
  });

  const runTool = useMutation({
    mutationFn: async (request: AgentToolRequest) =>
      unwrapResult(await commands.agentExecuteTool(request)),
  });

  const probeNetwork = useMutation({
    mutationFn: async (request: AgentNetworkProbeRequest) => {
      const result = unwrapResult(
        await commands.agentExecuteTool({
          tool: 'network.probe',
          arguments: request,
        }),
      );
      if (result.tool !== 'network.probe') {
        throw new Error('agent_tool_result_mismatch');
      }
      return result.output;
    },
  });

  const resolveIntent = useMutation({
    mutationFn: async (text: string) => commands.agentResolveIntent({ text }),
  });

  const propose = useMutation({
    mutationFn: async (action: AgentActionRequest) =>
      unwrapResult(await commands.agentProposeNetworkAction(action)),
  });

  const execute = useMutation({
    mutationFn: async ({ proposalId, digest }: AgentExecuteInput) =>
      unwrapResult(
        await commands.agentExecuteNetworkAction(proposalId, digest),
      ),
    onSuccess: (result) => {
      if (!result) {
        return;
      }

      queryClient.setQueryData<AgentNetworkSnapshot>(
        AGENT_NETWORK_SNAPSHOT_QUERY_KEY,
        result.snapshot,
      );
    },
  });

  const cancel = useMutation({
    mutationFn: async (proposalId: string) =>
      unwrapResult(await commands.agentCancelNetworkAction(proposalId)),
  });

  return {
    manifest,
    snapshot,
    auditHistory,
    diagnosticHistory,
    captureDiagnostic,
    runTool,
    probeNetwork,
    resolveIntent,
    propose,
    execute,
    cancel,
  };
};
