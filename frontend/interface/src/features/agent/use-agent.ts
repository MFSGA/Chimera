import { useMutation, useQuery, useQueryClient } from '@tanstack/react-query';
import {
  commands,
  type AgentActionRequest,
  type AgentNetworkSnapshot,
} from '../../ipc/bindings';
import { unwrapResult } from '../../utils';

export const AGENT_NETWORK_SNAPSHOT_QUERY_KEY = [
  'agent',
  'network-snapshot',
] as const;

export type AgentExecuteInput = {
  proposalId: string;
  digest: string;
};

/**
 * Provides the explicit-read and proposal lifecycle for the network agent.
 * The snapshot never polls or loads automatically; callers trigger `refetch`.
 */
export const useAgent = () => {
  const queryClient = useQueryClient();

  const snapshot = useQuery({
    queryKey: AGENT_NETWORK_SNAPSHOT_QUERY_KEY,
    queryFn: commands.agentGetNetworkSnapshot,
    enabled: false,
    retry: false,
    refetchInterval: false,
    refetchOnMount: false,
    refetchOnReconnect: false,
    refetchOnWindowFocus: false,
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
    snapshot,
    propose,
    execute,
    cancel,
  };
};
