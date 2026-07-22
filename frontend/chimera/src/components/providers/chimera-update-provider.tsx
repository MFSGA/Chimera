import { useSetting } from '@chimera/interface';
import { Update } from '@tauri-apps/plugin-updater';
import {
  createContext,
  PropsWithChildren,
  use,
  useEffect,
  useState,
} from 'react';
import packageJson from '@root/package.json';
import { checkUpdate, useUpdaterPlatformSupported } from '@/hooks/use-updater';
import { useBlockTask } from './block-task-provider';

const ChimeraUpdateContext = createContext<{
  currentVersion: string;
  hasNewVersion: boolean;
  newVersion: Update | null;
  isChecking: boolean;
  checkNewVersion: () => Promise<Update | null>;
  isSupported: boolean;
} | null>(null);

export const useChimeraUpdate = () => {
  const context = use(ChimeraUpdateContext);

  if (!context) {
    throw new Error(
      'useChimeraUpdate must be used within a ChimeraUpdateProvider',
    );
  }

  return context;
};

export default function ChimeraUpdateProvider({ children }: PropsWithChildren) {
  const { value: enableAutoCheckUpdate } = useSetting(
    'enable_auto_check_update',
  );
  const isSupported = useUpdaterPlatformSupported();
  const [hasNewVersion, setHasNewVersion] = useState(false);
  const [newVersion, setNewVersion] = useState<Update | null>(null);

  const blockTask = useBlockTask('check-chimera-update', async () => {
    const update = await checkUpdate();

    if (update) {
      setNewVersion(update);
      setHasNewVersion(true);
    }

    return update;
  });

  useEffect(() => {
    if (enableAutoCheckUpdate && isSupported) {
      void blockTask.execute().catch(console.error);
    }
    // oxlint-disable-next-line eslint-plugin-react-hooks/exhaustive-deps
  }, [enableAutoCheckUpdate, isSupported, blockTask.execute]);

  return (
    <ChimeraUpdateContext.Provider
      value={{
        currentVersion: packageJson.version,
        hasNewVersion,
        newVersion,
        isChecking: blockTask.isPending,
        checkNewVersion: blockTask.execute,
        isSupported,
      }}
    >
      {children}
    </ChimeraUpdateContext.Provider>
  );
}
