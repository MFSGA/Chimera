/**
 * Retry transient CDN failures with bounded exponential backoff. The prepare
 * task runner already retries five times, but doing so immediately makes
 * repeated HTTP 504 responses almost certain to exhaust the entire budget.
 * Rolling Alpha releases can briefly return 404 while assets are being
 * uploaded. Only their asset downloads receive a separate bounded backoff;
 * missing stable assets still fail without a delay.
 */
export function prepareRetryDelayMs(
  error: unknown,
  attempt: number,
  taskName?: string,
): number {
  if (!(error instanceof Error)) return 0;
  const rollingAlpha =
    taskName === 'mihomo-alpha' || taskName === 'clash-rs-alpha';
  if (rollingAlpha && /download failed:.*\(404\)/.test(error.message)) {
    return Math.min(1_000 * 2 ** Math.max(0, attempt), 8_000);
  }
  const transientHttp =
    /download failed:.*\((?:408|429|500|502|503|504)\)/.test(error.message);
  if (!transientHttp) return 0;
  return Math.min(2_000 * 2 ** Math.max(0, attempt), 16_000);
}
