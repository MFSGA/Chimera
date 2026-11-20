import { useKvStorage } from '@chimera/interface';
import { Button } from '@chimera/ui/button';
import {
  createContext,
  use,
  useCallback,
  useMemo,
  useState,
  type PropsWithChildren,
} from 'react';
import { DEFAULT_LAYOUTS } from '@/components/widgets/consts';
import * as m from '@/paraglide/messages';
import {
  applyDashboardLayoutEdit,
  createDashboardLayoutSaveQueue,
  type DashboardLayoutEdit,
  type DashboardLayoutSaveState,
} from './layout-save-model';
import {
  normalizeLayoutStorage,
  type DashboardLayoutStorage,
} from './layout-storage';

const DashboardLayoutContext = createContext<{
  storage: DashboardLayoutStorage;
  loading: boolean;
  readError: boolean;
  saveState: DashboardLayoutSaveState;
  saveEdit: (edit: DashboardLayoutEdit) => boolean;
  retry: () => void;
} | null>(null);

export function useDashboardLayoutPersistence() {
  const value = use(DashboardLayoutContext);
  if (!value) throw new Error('DashboardLayoutPersistenceProvider is missing');
  return value;
}

const DEFAULT_STORAGE = normalizeLayoutStorage(
  DEFAULT_LAYOUTS,
  DEFAULT_LAYOUTS,
);

export function DashboardLayoutPersistenceProvider({
  children,
}: PropsWithChildren) {
  const [storage, setStorage, { isLoading, readError, refresh }] = useKvStorage(
    'dashboard-widgets',
    DEFAULT_STORAGE,
    { migrate: (value) => normalizeLayoutStorage(value, DEFAULT_LAYOUTS) },
  );
  const [saveState, setSaveState] = useState<DashboardLayoutSaveState>('saved');
  const saver = useMemo(
    () =>
      createDashboardLayoutSaveQueue({
        write: (edit) =>
          setStorage((current) => applyDashboardLayoutEdit(current, edit)),
        refresh,
        onStateChange: setSaveState,
      }),
    [setStorage, refresh],
  );

  const saveEdit = useCallback(
    (edit: DashboardLayoutEdit) => {
      // Do not create an optimistic draft before the first authoritative read.
      return !isLoading && readError === null && saver.save(edit);
    },
    [isLoading, readError, saver],
  );

  const retry = useCallback(() => {
    if (isLoading || saver.getState() === 'saving') return;
    if (readError !== null) {
      void refresh();
      return;
    }
    saver.retry();
  }, [isLoading, readError, refresh, saver]);

  return (
    <DashboardLayoutContext.Provider
      value={{
        storage,
        loading: isLoading,
        readError: readError !== null,
        saveState,
        saveEdit,
        retry,
      }}
    >
      {children}
    </DashboardLayoutContext.Provider>
  );
}

export function DashboardLayoutSaveStatus() {
  const { loading, readError, saveState, retry } =
    useDashboardLayoutPersistence();
  const failed = readError || saveState === 'error';
  if (!loading && saveState === 'saved' && !failed) return null;

  return (
    <div
      className="text-on-surface-variant flex items-center justify-between gap-2 text-xs"
      data-slot="dashboard-layout-save-status"
      data-save-state={
        loading ? 'loading' : readError ? 'load-error' : saveState
      }
    >
      <span
        role="status"
        aria-live="polite"
        className={failed ? 'text-error' : undefined}
      >
        {loading
          ? m.dashboard_layout_loading()
          : readError
            ? m.dashboard_layout_load_failed()
            : saveState === 'saving'
              ? m.dashboard_layout_saving()
              : m.dashboard_layout_save_failed()}
      </span>
      {failed && (
        <Button
          className="h-7 min-w-0 px-2 text-xs"
          disabled={loading || saveState === 'saving'}
          onClick={retry}
        >
          {m.dashboard_widget_config_retry()}
        </Button>
      )}
    </div>
  );
}
