/**
 * Retry transient CDN failures with bounded exponential backoff. The prepare
 * task runner already retries five times, but doing so immediately makes
 * repeated HTTP 504 responses almost certain to exhaust the entire budget.
 * Non-transient failures such as 404 retain the existing retry behavior.
 */
export function prepareRetryDelayMs(error: unknown, attempt: number): number {
  if (!(error instanceof Error)) return 0;
  const transientHttp =
    /download failed:.*\((?:408|429|500|502|503|504)\)/.test(error.message);
  if (!transientHttp) return 0;
  return Math.min(2_000 * 2 ** Math.max(0, attempt), 16_000);
}
