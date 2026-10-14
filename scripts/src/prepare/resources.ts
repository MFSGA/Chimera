import type { BinInfo } from './types.ts';

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
