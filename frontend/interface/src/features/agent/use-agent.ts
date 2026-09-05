import { useMutation, useQuery, useQueryClient } from '@tanstack/react-query';
import {
  commands,
  type AgentActionRequest,
  type AgentNetworkSnapshot,
  type AgentToolName,
} from '../../ipc/bindings';
import { unwrapResult } from '../../utils';

export const AGENT_MANIFEST_QUERY_KEY = ['agent', 'manifest'] as const;

export const AGENT_NETWORK_SNAPSHOT_QUERY_KEY = [
  'agent',
  'network-snapshot',
] as const;

export type AgentExecuteInput = {
  proposalId: string;
  digest: string;
};

const getSystemSnapshot = async (): Promise<AgentNetworkSnapshot> => {
  const result = unwrapResult(
    await commands.agentExecuteReadonlyTool('system.snapshot'),
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

  const runTool = useMutation({
    mutationFn: async (tool: AgentToolName) =>
      unwrapResult(await commands.agentExecuteReadonlyTool(tool)),
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
    runTool,
    propose,
    execute,
    cancel,
  };
};
