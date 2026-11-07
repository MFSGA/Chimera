import { useMutation, useQueryClient } from '@tanstack/react-query';
import { useCallback } from 'react';
import { unwrapResult } from '../utils/index.js';
import type {
  DelayHistory,
  NativeProxies_Serialize,
  Proxy_Serialize,
} from './bindings.js';
import { mutations, queries } from './query-bindings.js';
import { invokeMutation, invokeQuery } from './query-options.js';

/**
 * Change only the tested native node. A group holds its member's name rather
 * than another copy, so every group sees the same updated history.
 */
export const withNativeDelaySample = (
  graph: NativeProxies_Serialize | undefined,
  name: string,
  delay: number,
  url: string | null,
): NativeProxies_Serialize | undefined => {
  const node = graph?.nodes[name];
  if (!graph || !node) return graph;

  const sample = {
    time: new Date().toISOString(),
    delay,
  } satisfies DelayHistory;

  let updated: Proxy_Serialize = {
    ...node,
    history: [...node.history, sample],
  };

  if (url) {
    const extra = node.extra?.[url];
    updated = {
      ...updated,
      extra: {
        ...node.extra,
        [url]: {
          alive: delay > 0,
          history: [...(extra?.history ?? []), sample],
        },
      },
    };
  }

  return {
    ...graph,
    nodes: { ...graph.nodes, [name]: updated },
  };
};

/**
 * Main native proxy view actions. No action closure is stored in Query data.
 * The legacy UI continues to use its compatibility hook and command schema.
 *
 * Provider routing and expected status are passed to the core as typed
 * delay parameters; the legacy UI may omit both without changing its behavior.
 */
export const useNativeProxyActions = () => {
  const queryClient = useQueryClient();
  const nativeKey = queries.getNativeProxies().queryKey;
  const legacyKey = queries.getProxies().queryKey;

  const { mutateAsync: performSelect } = useMutation({
    mutationKey: mutations.selectProxy.mutationKey,
    mutationFn: async ([group, name]: [string, string]) =>
      unwrapResult(await invokeMutation(mutations.selectProxy, [group, name])),
    onSuccess: async () => {
      await Promise.all([
        queryClient.refetchQueries({ queryKey: nativeKey, exact: true }),
        queryClient.invalidateQueries({ queryKey: legacyKey, exact: true }),
      ]);
    },
  });

  const selectProxy = useCallback(
    (group: string, name: string) => performSelect([group, name]),
    [performSelect],
  );

  const updateProxiesDelay = useMutation({
    mutationFn: async ([name, url, provider, expected]: [
      string,
      string | null,
      string | null,
      string | null,
    ]) => {
      const result = unwrapResult(
        await invokeQuery(
          queries.clashApiGetProxyDelay(name, url, provider, expected),
        ),
      );
      return {
        name,
        url,
        delay: result?.delay ?? 0,
      };
    },
    onSuccess: ({ name, url, delay }) => {
      queryClient.setQueryData<NativeProxies_Serialize | undefined>(
        nativeKey,
        (graph) => withNativeDelaySample(graph, name, delay, url),
      );
      void queryClient
        .invalidateQueries({ queryKey: legacyKey, exact: true })
        .catch(console.error);
    },
  });

  return { selectProxy, updateProxiesDelay };
};
