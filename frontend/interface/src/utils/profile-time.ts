const MAX_JAVASCRIPT_DATE_MILLISECONDS = 8_640_000_000_000_000;
const MAX_JAVASCRIPT_DATE_SECONDS = MAX_JAVASCRIPT_DATE_MILLISECONDS / 1000;

function isPositiveSafeInteger(
  value: number | null | undefined,
): value is number {
  return typeof value === 'number' && Number.isSafeInteger(value) && value > 0;
}

export function getSafeProfileTimestamp(
  value: number | null | undefined,
): number | null {
  return isPositiveSafeInteger(value) && value <= MAX_JAVASCRIPT_DATE_SECONDS
    ? value
    : null;
}

export function getNextProfileUpdateTimestamp(
  updatedAt: number | null | undefined,
  updateIntervalMinutes: number | null | undefined,
): number | null {
  const safeUpdatedAt = getSafeProfileTimestamp(updatedAt);
  if (safeUpdatedAt === null || !isPositiveSafeInteger(updateIntervalMinutes)) {
    return null;
  }

  const intervalSeconds = updateIntervalMinutes * 60;
  if (!Number.isSafeInteger(intervalSeconds)) return null;

  const nextUpdate = safeUpdatedAt + intervalSeconds;
  if (
    !Number.isSafeInteger(nextUpdate) ||
    nextUpdate > MAX_JAVASCRIPT_DATE_SECONDS
  ) {
    return null;
  }

  return nextUpdate;
}
