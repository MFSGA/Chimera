import { useQuery } from '@tanstack/react-query';
import { queries } from './query-bindings.js';
import { useNativeProxies } from './use-native-proxies.js';

const NO_ICONS: Readonly<Record<string, string>> = Object.freeze({});

/**
 * Per-group icon URLs, separate from the ordinary native proxy listing.
 * Mirrors ref's read-channel split. Chimera's desktop IPC does not yet
 * implement ref's per-scope sensitive_read authorization.
 */
export const useProxyGroupIcons = (): Readonly<Record<string, string>> => {
  // Refetch icon URLs when the native graph changes (including a Profile
  // activation). A stale icon URL must not carry over to another profile.
  const native = useNativeProxies();
  const options = queries.getProxyGroupIcons();
  const query = useQuery({
    ...options,
    queryKey: [...options.queryKey, String(native.dataUpdatedAt)],
    enabled: native.isSuccess,
    retry: false,
    gcTime: 60_000,
  });
  return query.data ?? NO_ICONS;
};
