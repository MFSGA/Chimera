import { useMutation, useQuery, useQueryClient } from '@tanstack/react-query';
import {
  commands,
  type AgentActionRequest,
  type AgentNetworkSnapshot,
} from '../../ipc/bindings';
import { unwrapResult } from '../../utils';
import {
  AGENT_HISTORY_QUERY_KEY,
  AGENT_NETWORK_SNAPSHOT_QUERY_KEY,
} from './query-keys';

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
    queryFn: async () => unwrapResult(await commands.agentGetNetworkSnapshot()),
    enabled: false,
    retry: false,
    refetchInterval: false,
    refetchOnMount: false,
    refetchOnReconnect: false,
    refetchOnWindowFocus: false,
  });

  const history = useQuery({
    queryKey: AGENT_HISTORY_QUERY_KEY,
    queryFn: async () => unwrapResult(await commands.agentGetHistory()),
    retry: false,
    refetchInterval: false,
    refetchOnReconnect: false,
    refetchOnWindowFocus: false,
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
      void queryClient.invalidateQueries({ queryKey: AGENT_HISTORY_QUERY_KEY });
    },
  });

  const cancel = useMutation({
    mutationFn: async (proposalId: string) =>
      unwrapResult(await commands.agentCancelNetworkAction(proposalId)),
  });

  const clearHistory = useMutation({
    mutationFn: async () => unwrapResult(await commands.agentClearHistory()),
    onSuccess: (result) => {
      if (result) {
        queryClient.setQueryData(AGENT_HISTORY_QUERY_KEY, result);
      }
    },
  });

  return {
    snapshot,
    history,
    resolveIntent,
    propose,
    execute,
    cancel,
    clearHistory,
  };
};
