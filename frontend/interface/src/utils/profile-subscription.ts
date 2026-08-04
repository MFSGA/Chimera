export interface ProfileSubscriptionUsage {
  progress: number;
  total: number;
  used: number;
}

function normalizeSubscriptionCounter(
  value: number | null | undefined,
): number {
  return typeof value === 'number' && Number.isSafeInteger(value) && value >= 0
    ? value
    : 0;
}

export function getProfileSubscriptionUsage(
  upload: number | null | undefined,
  download: number | null | undefined,
  total: number | null | undefined,
): ProfileSubscriptionUsage {
  const safeUpload = normalizeSubscriptionCounter(upload);
  const safeDownload = normalizeSubscriptionCounter(download);
  const safeTotal = normalizeSubscriptionCounter(total);
  const sum = safeUpload + safeDownload;
  const used = Number.isSafeInteger(sum) ? sum : 0;
  const progress =
    safeTotal > 0 ? Math.min(100, Math.max(0, (used / safeTotal) * 100)) : 0;

  return { progress, total: safeTotal, used };
}
