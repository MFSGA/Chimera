import type { BinInfo } from './types.ts';

export interface SidecarRefreshRetryPolicy {
  delaysMs: readonly number[];
  shouldRetry: (error: unknown) => boolean;
  sleep?: (delayMs: number) => Promise<void>;
}

async function retryRefreshedSidecar<T>(
  info: BinInfo,
  materialize: (binInfo: BinInfo) => Promise<T>,
  lastError: unknown,
  debugLog: (message: string) => void,
  policy: SidecarRefreshRetryPolicy,
): Promise<T> {
  for (const delayMs of policy.delaysMs) {
    if (!policy.shouldRetry(lastError)) throw lastError;

    await (
      policy.sleep ??
      ((delay) => new Promise((resolve) => setTimeout(resolve, delay)))
    )(delayMs);

    try {
      const refreshed = await info.refresh!();
      debugLog(`retrying ${info.name} with version ${refreshed.version}`);
      return await materialize(refreshed);
    } catch (error) {
      if (!policy.shouldRetry(error)) throw error;
      lastError = error;
    }
  }

  throw lastError;
}

export async function resolveSidecar<T>(
  binInfo: BinInfo | Promise<BinInfo>,
  materialize: (binInfo: BinInfo) => Promise<T>,
  debugLog: (message: string) => void = () => {},
  refreshRetry?: SidecarRefreshRetryPolicy,
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
      if (refreshRetry && refreshRetry.shouldRetry(refreshError)) {
        return retryRefreshedSidecar(
          info,
          materialize,
          refreshError,
          debugLog,
          refreshRetry,
        );
      }
      throw error;
    }

    debugLog(`retrying ${info.name} with version ${refreshed.version}`);
    try {
      return await materialize(refreshed);
    } catch (refreshedError) {
      if (!refreshRetry || !refreshRetry.shouldRetry(refreshedError)) {
        throw refreshedError;
      }
      return retryRefreshedSidecar(
        info,
        materialize,
        refreshedError,
        debugLog,
        refreshRetry,
      );
    }
  }
}
