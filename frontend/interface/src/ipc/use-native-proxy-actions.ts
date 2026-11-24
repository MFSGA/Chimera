import { useMutation, useQueryClient } from '@tanstack/react-query';
import { useCallback } from 'react';
import { unwrapResult } from '../utils/index.js';
import type {
  DelayHistory,
  Proxies_Serialize,
  Proxy_Serialize,
} from './bindings.js';
import { mutations, queries } from './query-bindings.js';
import { invokeMutation, invokeQuery } from './query-options.js';

/**
 * Change only the tested native node. A group holds its member's name rather
 * than another copy, so every group sees the same updated history.
 */
export const withNativeDelaySample = (
  graph: Proxies_Serialize | undefined,
  name: string,
  delay: number,
  delayKey: string,
): Proxies_Serialize | undefined => {
  const node = graph?.nodes[name];
  if (!graph || !node) return graph;

  const sample = {
    time: new Date().toISOString(),
    delay,
  } satisfies DelayHistory;

  const extra = node.extra?.[delayKey];
  const updated: Proxy_Serialize = {
    ...node,
    history: [...node.history, sample],
    extra: {
      ...node.extra,
      [delayKey]: {
        alive: delay > 0,
        history: [...(extra?.history ?? []), sample],
      },
    },
  };

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

  const { mutateAsync: performClearFixed, isPending: isClearingFixed } =
    useMutation({
      mutationKey: mutations.clearProxyFixed.mutationKey,
      mutationFn: async ([group]: [string]) =>
        unwrapResult(await invokeMutation(mutations.clearProxyFixed, [group])),
      // A timeout or dropped IPC reply does not prove the DELETE failed.
      // Re-read both views even on errors; never optimistically clear fixed.
      onSettled: async () => {
        await Promise.allSettled([
          queryClient.refetchQueries({ queryKey: nativeKey, exact: true }),
          queryClient.invalidateQueries({ queryKey: legacyKey, exact: true }),
        ]);
      },
    });

  const clearProxyFixed = useCallback(
    (group: string) => performClearFixed([group]),
    [performClearFixed],
  );

  const updateProxiesDelay = useMutation({
    mutationKey: mutations.testNativeProxyNodeDelay.mutationKey,
    mutationFn: async ([group, name]: [string, string]) =>
      unwrapResult(
        await invokeMutation(mutations.testNativeProxyNodeDelay, [group, name]),
      ),
    // The backend resolves the URL and checks membership against the live core.
    // Re-read its real history even if the IPC reply was lost or timed out.
    onSettled: async () => {
      await Promise.allSettled([
        queryClient.refetchQueries({ queryKey: nativeKey, exact: true }),
        queryClient.invalidateQueries({ queryKey: legacyKey, exact: true }),
      ]);
    },
  });

  const { mutateAsync: testGroupDelay, isPending: isTestingGroupDelay } =
    useMutation({
      mutationKey: mutations.testNativeProxyGroupDelay.mutationKey,
      mutationFn: async (groupName: string) =>
        unwrapResult(
          await invokeMutation(mutations.testNativeProxyGroupDelay, [
            groupName,
          ]),
        ) ?? {},
      onSettled: async () => {
        // The controller records the per-node history. Read its real state,
        // including after partial failures, instead of writing fake samples.
        await Promise.allSettled([
          queryClient.refetchQueries({ queryKey: nativeKey, exact: true }),
          queryClient.invalidateQueries({ queryKey: legacyKey, exact: true }),
        ]);
      },
    });

  return {
    selectProxy,
    clearProxyFixed,
    isClearingFixed,
    updateProxiesDelay,
    testGroupDelay,
    isTestingGroupDelay,
  };
};
