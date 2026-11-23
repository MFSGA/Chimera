import type {
  ClashConnectionsSnapshot,
  ClashTraffic,
} from '@chimera/interface';

/** Adapt Chimera's controller DTOs to the ref Dashboard sparkline model. */
export const padSparklineData = (
  data: (number | undefined)[] = [],
  max: number,
): number[] =>
  Array(Math.max(0, max - data.length))
    .fill(0)
    .concat(data.slice(-max))
    .map((value: number | undefined) => value ?? 0);

export function trafficSamples(
  traffic: readonly ClashTraffic[] | undefined,
  direction: 'up' | 'down',
  samples: number,
): number[] {
  return padSparklineData(
    traffic?.map((snapshot) => snapshot[direction]),
    samples,
  );
}

export function connectionCount(
  snapshot: ClashConnectionsSnapshot | undefined,
): number {
  return snapshot?.connections?.length ?? 0;
}

export function connectionSamples(
  snapshots: readonly ClashConnectionsSnapshot[] | undefined,
  samples: number,
): number[] {
  return padSparklineData(snapshots?.map(connectionCount), samples);
}
