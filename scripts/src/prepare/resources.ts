import type { BinInfo } from './types.ts';

export function isVersionManagedTarget(name: string): boolean {
  return ['chimera-service', 'chimera-client-alpha', 'mihomo-alpha'].includes(
    name,
  );
}

export function isSidecarCacheValid(
  name: string,
  targetExists: boolean,
  versionStamp: string | null,
  version?: string,
): boolean {
  if (!targetExists) return false;
  return !isVersionManagedTarget(name) || versionStamp === version;
}

export async function resolveSidecar<T>(
  binInfo: BinInfo | Promise<BinInfo>,
  materialize: (binInfo: BinInfo) => Promise<T>,
  debugLog: (message: string) => void = () => {},
): Promise<T> {
  const info = await binInfo;
  try {
    return await materialize(info);
  } catch (error) {
    if (!info.refresh) throw error;

    let refreshed: BinInfo;
    try {
      refreshed = await info.refresh();
    } catch (refreshError) {
      debugLog(`failed to refresh ${info.name}: ${String(refreshError)}`);
      throw error;
    }

    debugLog(`retrying ${info.name} with version ${refreshed.version}`);
    return await materialize(refreshed);
  }
}
