import { useAtomValue, useSetAtom } from 'jotai';
import { useEffect } from 'react';
import { useChimeraUpdate } from '@/components/providers/chimera-update-provider';
import { UpdaterIgnoredAtom, UpdaterInstanceAtom } from '@/store/updater';

export const LegacyUpdaterAdapter = () => {
  const { hasNewVersion, snapshot } = useChimeraUpdate();
  const updaterIgnored = useAtomValue(UpdaterIgnoredAtom);
  const setUpdaterInstance = useSetAtom(UpdaterInstanceAtom);

  useEffect(() => {
    const version = snapshot?.release?.version;
    if (hasNewVersion && snapshot && version && updaterIgnored !== version) {
      setUpdaterInstance(snapshot);
    }
  }, [hasNewVersion, snapshot, setUpdaterInstance, updaterIgnored]);

  return null;
};
