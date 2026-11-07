import { useQuery } from '@tanstack/react-query';
import type { NativeProxies_Serialize } from './bindings.js';
import { queries } from './query-bindings.js';
import { unwrapQueryOptions } from './query-options.js';

/** The native wire graph keeps a single node record even if groups share it. */
export const selectVisibleNativeProxies = (
  graph: NativeProxies_Serialize,
): NativeProxies_Serialize => {
  const visibleGroups = graph.groups.filter((group) => !group.hidden);
  return visibleGroups.length === graph.groups.length
    ? graph
    : { ...graph, groups: visibleGroups };
};

/**
 * Normalized graph for new main-UI consumers. Query data contains no action
 * closures; legacy main/legacy components still use their existing hook until
 * their group/node actions are migrated alongside the backend API contract.
 * Hidden groups stay in the underlying cache but are omitted from this view.
 */
export const useNativeProxies = () => {
  const native = queries.getNativeProxies();
  return useQuery({
    ...unwrapQueryOptions(native, native.queryFn!),
    select: selectVisibleNativeProxies,
  });
};
