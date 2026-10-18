import { useKvStorage } from '@chimera/interface';
import { useDndGridContext } from '@chimera/ui/dnd-grid';
import {
  createContext,
  use,
  useCallback,
  useRef,
  useState,
  type PropsWithChildren,
} from 'react';
import {
  DEFAULT_WIDGET_CONFIGS,
  EMPTY_WIDGET_CONFIG_STORAGE,
  getWidgetConfig,
  normalizeWidgetConfigStorage,
  type WidgetConfig,
  type WidgetConfigStorage,
  type WidgetId,
} from './widget-config';

type SaveStatus = { state: 'saved' | 'saving' | 'error'; id?: string };

const DashboardContext = createContext<{
  openSheet: boolean;
  setOpenSheet: (open: boolean) => void;
  isEditing: boolean;
  setIsEditing: (editing: boolean) => void;
  configs: WidgetConfigStorage;
  saveConfig: (id: string, config: WidgetConfig) => void;
  saveStatus: SaveStatus;
  configLoading: boolean;
  configReadError: boolean;
  retryConfig: () => void;
} | null>(null);

export const useDashboardContext = () => {
  const context = use(DashboardContext);

  if (!context) {
    throw new Error(
      'useDashboardContext must be used within a DashboardProvider',
    );
  }

  return context;
};

export function DashboardProvider({ children }: PropsWithChildren) {
  const [openSheet, setOpenSheet] = useState(false);
  const [isEditing, setIsEditing] = useState(false);
  const [configs, setConfigs, { isLoading, readError, refresh }] = useKvStorage(
    'dashboard-widget-configs',
    EMPTY_WIDGET_CONFIG_STORAGE,
    { migrate: normalizeWidgetConfigStorage },
  );
  const [saveStatus, setSaveStatus] = useState<SaveStatus>({ state: 'saved' });

  const configsRef = useRef(configs);
  configsRef.current = configs;
  const saveSequenceRef = useRef(0);

  const persist = useCallback(
    async (id: string | undefined, next: WidgetConfigStorage) => {
      const sequence = ++saveSequenceRef.current;
      setSaveStatus({ state: 'saving', id });
      let saved = false;
      try {
        saved = await setConfigs(next);
      } catch {
        saved = false;
      }
      if (sequence === saveSequenceRef.current) {
        setSaveStatus({ state: saved ? 'saved' : 'error', id });
      }
    },
    [setConfigs],
  );

  const saveConfig = useCallback(
    (id: string, config: WidgetConfig) => {
      if (isLoading || readError !== null) return;
      const next: WidgetConfigStorage = {
        version: 1,
        byInstance: { ...configsRef.current.byInstance, [id]: config },
      };
      configsRef.current = next;
      void persist(id, next);
    },
    [isLoading, readError, persist],
  );

  const retryConfig = useCallback(() => {
    if (readError !== null) void refresh();
    else void persist(saveStatus.id, configsRef.current);
  }, [readError, refresh, persist, saveStatus.id]);

  return (
    <DashboardContext.Provider
      value={{
        openSheet,
        setOpenSheet,
        isEditing,
        setIsEditing,
        configs,
        saveConfig,
        saveStatus,
        configLoading: isLoading,
        configReadError: readError !== null,
        retryConfig,
      }}
    >
      {children}
    </DashboardContext.Provider>
  );
}

export function useWidgetConfig<T extends WidgetId>(id: string, type: T) {
  const { configs } = useDashboardContext();
  const { sourceOnly, isOverlay, dragIdPrefix } = useDndGridContext();

  return sourceOnly && (!isOverlay || dragIdPrefix !== '')
    ? DEFAULT_WIDGET_CONFIGS[type]
    : getWidgetConfig(configs, id, type);
}
