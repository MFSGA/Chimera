import type { ProfileSubscriptionInfo } from '@chimera/interface';

export type SubscriptionQuota = {
  upload: number;
  download: number;
  used: number;
  total: number;
  remaining: number;
  usedPercent: number;
  remainingPercent: number;
  overage: number;
};

const isFiniteNonNegative = (
  value: number | null | undefined,
): value is number =>
  typeof value === 'number' && Number.isFinite(value) && value >= 0;

/** Missing directions or an invalid/zero total remain unknown, not zero. */
export function calculateSubscriptionQuota(
  subscription?: ProfileSubscriptionInfo | null,
): SubscriptionQuota | null {
  const upload = subscription?.upload;
  const download = subscription?.download;
  const total = subscription?.total;
  if (
    !subscription ||
    !isFiniteNonNegative(upload) ||
    !isFiniteNonNegative(download) ||
    !isFiniteNonNegative(total) ||
    total === 0
  ) {
    return null;
  }

  const used = upload + download;
  const usedPercent = (used / total) * 100;

  return {
    upload,
    download,
    used,
    total,
    remaining: Math.max(0, total - used),
    usedPercent: Math.min(100, usedPercent),
    remainingPercent: Math.max(0, 100 - usedPercent),
    overage: Math.max(0, used - total),
  };
}

export function subscriptionExpiryTimestamp(
  expire?: number | null,
): number | null {
  if (typeof expire !== 'number' || !Number.isFinite(expire) || expire <= 0) {
    return null;
  }
  const timestamp = expire * 1000;
  return Number.isFinite(timestamp) &&
    Number.isFinite(new Date(timestamp).getTime())
    ? timestamp
    : null;
}
