import { useMutation, useQuery, useQueryClient } from '@tanstack/react-query';
import { commands, type AgentBridgeStatus } from '../../ipc/bindings';
import { unwrapResult } from '../../utils';
import {
  AGENT_BRIDGE_STATUS_QUERY_KEY,
  AGENT_MANIFEST_QUERY_KEY,
} from './query-keys';

/**
 * Provides the local Agent Bridge lifecycle and its shared tool manifest.
 * The bearer token is only returned from the start mutation and is never
 * copied into the query cache.
 */
export const useAgentBridge = () => {
  const queryClient = useQueryClient();

  const status = useQuery({
    queryKey: AGENT_BRIDGE_STATUS_QUERY_KEY,
    queryFn: async () => unwrapResult(await commands.getAgentBridgeStatus()),
    retry: false,
    refetchInterval: 5_000,
    refetchIntervalInBackground: true,
    refetchOnWindowFocus: false,
  });

  const manifest = useQuery({
    queryKey: AGENT_MANIFEST_QUERY_KEY,
    queryFn: commands.agentGetManifest,
    staleTime: Number.POSITIVE_INFINITY,
    retry: false,
    refetchOnWindowFocus: false,
  });

  const start = useMutation({
    mutationFn: async () => unwrapResult(await commands.startAgentBridge()),
    onSuccess: (result) => {
      if (!result) return;
      queryClient.setQueryData<AgentBridgeStatus>(
        AGENT_BRIDGE_STATUS_QUERY_KEY,
        {
          running: result.running,
          base_url: result.base_url,
        },
      );
    },
  });

  const stop = useMutation({
    mutationFn: async () => unwrapResult(await commands.stopAgentBridge()),
    onSuccess: (result) => {
      if (!result) return;
      queryClient.setQueryData<AgentBridgeStatus>(
        AGENT_BRIDGE_STATUS_QUERY_KEY,
        result,
      );
    },
  });

  return {
    status,
    manifest,
    start,
    stop,
  };
};
