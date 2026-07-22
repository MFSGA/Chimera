import {
  commands,
  unwrapResult,
  useIsAppImage,
  useSetting,
} from '@chimera/interface';
import { Update } from '@tauri-apps/plugin-updater';
import {
  createContext,
  PropsWithChildren,
  use,
  useEffect,
  useState,
} from 'react';
import packageJson from '@root/package.json';
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
  const { data: isAppImage } = useIsAppImage();
  const isSupported = !isAppImage || !WIN_PORTABLE;
  const [hasNewVersion, setHasNewVersion] = useState(false);
  const [newVersion, setNewVersion] = useState<Update | null>(null);

  const blockTask = useBlockTask('check-chimera-update', async () => {
    const metadata = unwrapResult(await commands.checkUpdate());

    if (metadata) {
      const update = new Update({
        rid: metadata.rid,
        currentVersion: metadata.current_version,
        version: metadata.version,
        rawJson: metadata.raw_json as Record<string, unknown>,
      });

      setNewVersion(update);
      setHasNewVersion(true);

      return update;
    }

    return null;
  });

  useEffect(() => {
    if (enableAutoCheckUpdate) {
      blockTask.execute();
    }
    // oxlint-disable-next-line eslint-plugin-react-hooks/exhaustive-deps
  }, [enableAutoCheckUpdate, blockTask.execute]);

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
