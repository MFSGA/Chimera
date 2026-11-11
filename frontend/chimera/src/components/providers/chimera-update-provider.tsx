import {
  commands,
  events,
  unwrapResult,
  type AppUpdatePhase,
  type AppUpdateSnapshot,
} from '@chimera/interface';
import {
  createContext,
  PropsWithChildren,
  use,
  useCallback,
  useEffect,
  useRef,
  useState,
} from 'react';
import packageJson from '@root/package.json';

const terminalCheckPhases: AppUpdatePhase[] = [
  'up_to_date',
  'available',
  'ready',
  'failed',
];

const ChimeraUpdateContext = createContext<{
  currentVersion: string;
  snapshot: AppUpdateSnapshot | null;
  hasNewVersion: boolean;
  isChecking: boolean;
  isBusy: boolean;
  isSupported: boolean;
  checkNewVersion: () => Promise<AppUpdateSnapshot>;
  downloadUpdate: () => Promise<AppUpdateSnapshot>;
  cancelDownload: () => Promise<AppUpdateSnapshot>;
  installUpdate: () => Promise<AppUpdateSnapshot>;
  discardUpdate: () => Promise<AppUpdateSnapshot>;
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
  const [snapshot, setSnapshot] = useState<AppUpdateSnapshot | null>(null);
  const snapshotRef = useRef<AppUpdateSnapshot | null>(null);

  const publish = useCallback((next: AppUpdateSnapshot) => {
    const previous = snapshotRef.current;
    if (previous && next.revision < previous.revision) return;
    snapshotRef.current = next;
    setSnapshot(next);
  }, []);

  useEffect(() => {
    let active = true;
    const unlistenPromise = events.appUpdateStateChanged.listen(
      ({ payload }) => {
        if (active) publish(payload);
      },
    );

    void commands
      .getAppUpdateState()
      .then(unwrapResult)
      .then((next) => {
        if (active) publish(next);
      })
      .catch((error) =>
        console.error('failed to load application update state', error),
      );

    return () => {
      active = false;
      void unlistenPromise.then((unlisten) => unlisten());
    };
  }, [publish]);

  const checkNewVersion = useCallback(async () => {
    const baseline = snapshotRef.current?.revision ?? 0;
    let finish!: (value: AppUpdateSnapshot) => void;
    const completed = new Promise<AppUpdateSnapshot>((resolve) => {
      finish = resolve;
    });
    const unlisten = await events.appUpdateStateChanged.listen(
      ({ payload }) => {
        publish(payload);
        if (
          payload.revision > baseline &&
          terminalCheckPhases.includes(payload.phase)
        ) {
          finish(payload);
        }
      },
    );
    let timeoutId: number | undefined;

    try {
      const started = unwrapResult(await commands.checkAppUpdate());
      publish(started);
      if (terminalCheckPhases.includes(started.phase)) return started;

      return await Promise.race([
        completed,
        new Promise<AppUpdateSnapshot>((resolve) => {
          timeoutId = window.setTimeout(() => {
            const current = snapshotRef.current;
            if (current) resolve(current);
            else resolve(started);
          }, 45_000);
        }),
      ]);
    } finally {
      unlisten();
      if (timeoutId !== undefined) window.clearTimeout(timeoutId);
    }
  }, [publish]);

  const runCommand = useCallback(
    async (command: () => ReturnType<typeof commands.downloadAppUpdate>) => {
      const next = unwrapResult(await command());
      publish(next);
      return next;
    },
    [publish],
  );

  const downloadUpdate = useCallback(
    () => runCommand(commands.downloadAppUpdate),
    [runCommand],
  );
  const cancelDownload = useCallback(
    () => runCommand(commands.cancelAppUpdateDownload),
    [runCommand],
  );
  const installUpdate = useCallback(
    () => runCommand(commands.installAppUpdate),
    [runCommand],
  );
  const discardUpdate = useCallback(
    () => runCommand(commands.discardAppUpdatePackage),
    [runCommand],
  );

  const isSupported = snapshot?.supported ?? !WIN_PORTABLE;
  const isBusy =
    snapshot != null &&
    [
      'checking',
      'downloading',
      'cancelling',
      'verifying',
      'installing',
    ].includes(snapshot.phase);

  return (
    <ChimeraUpdateContext.Provider
      value={{
        currentVersion: packageJson.version,
        snapshot,
        hasNewVersion: snapshot?.release != null,
        isChecking: snapshot?.phase === 'checking',
        isBusy,
        isSupported,
        checkNewVersion,
        downloadUpdate,
        cancelDownload,
        installUpdate,
        discardUpdate,
      }}
    >
      {children}
    </ChimeraUpdateContext.Provider>
  );
}
