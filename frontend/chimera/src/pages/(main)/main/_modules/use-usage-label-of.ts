import { useProfile, type Dimension } from '@chimera/interface';
import { useCallback, useMemo } from 'react';
import { usageLabel } from '@/utils/traffic-usage';

/** How a group's key reads on screen, with profile uids named once loaded. */
export function useUsageLabelOf() {
  const {
    query: { data: profiles },
  } = useProfile();

  const profileNames = useMemo(
    () =>
      profiles && new Map(profiles.items.map((item) => [item.uid, item.name])),
    [profiles],
  );

  return useCallback(
    (dimension: Dimension, key: string) =>
      usageLabel(dimension, key, profileNames),
    [profileNames],
  );
}
