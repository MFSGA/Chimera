import { useClashConnectionDetails } from '@chimera/interface';
import { useMemo } from 'react';
import {
  latestGroupTrafficSpeed,
  type GroupTrafficSpeed,
} from './group-traffic-speed';

const ZERO_SPEED: GroupTrafficSpeed = { download: 0, upload: 0 };

/** Live speed of all connections passing through the given group. */
export function useGroupTrafficSpeed(groupName?: string): GroupTrafficSpeed {
  const { data } = useClashConnectionDetails();

  return useMemo(
    () =>
      groupName && data
        ? latestGroupTrafficSpeed([data], groupName)
        : ZERO_SPEED,
    [data, groupName],
  );
}
