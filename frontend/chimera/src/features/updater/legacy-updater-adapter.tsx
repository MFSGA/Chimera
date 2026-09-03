import { useAtomValue, useSetAtom } from 'jotai';
import { useEffect } from 'react';
import { useChimeraUpdate } from '@/components/providers/chimera-update-provider';
import { UpdaterIgnoredAtom, UpdaterInstanceAtom } from '@/store/updater';

export const LegacyUpdaterAdapter = () => {
  const { hasNewVersion, newVersion } = useChimeraUpdate();
  const updaterIgnored = useAtomValue(UpdaterIgnoredAtom);
  const setUpdaterInstance = useSetAtom(UpdaterInstanceAtom);

  useEffect(() => {
    if (hasNewVersion && newVersion && updaterIgnored !== newVersion.version) {
      setUpdaterInstance(newVersion);
    }
  }, [hasNewVersion, newVersion, setUpdaterInstance, updaterIgnored]);

  return null;
};
