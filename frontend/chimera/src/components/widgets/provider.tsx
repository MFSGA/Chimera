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
  withWidgetConfig,
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

  const saveSequenceRef = useRef(0);
  // Save the last *intent*, not a whole-map snapshot: the backend may have
  // received another instance's configuration while this write was in flight.
  const retryIntentRef = useRef<{ id: string; config: WidgetConfig } | null>(
    null,
  );

  const persist = useCallback(
    async (id: string, config: WidgetConfig) => {
      const sequence = ++saveSequenceRef.current;
      setSaveStatus({ state: 'saving', id });
      let saved = false;
      try {
        saved = await setConfigs((current) =>
          withWidgetConfig(current, id, config),
        );
      } catch {
        saved = false;
      }
      if (sequence === saveSequenceRef.current) {
        if (saved) retryIntentRef.current = null;
        setSaveStatus({ state: saved ? 'saved' : 'error', id });
      }
    },
    [setConfigs],
  );

  const saveConfig = useCallback(
    (id: string, config: WidgetConfig) => {
      // Never overwrite a pending authoritative read or an unreadable value.
      if (isLoading || readError !== null) return;
      retryIntentRef.current = { id, config };
      void persist(id, config);
    },
    [isLoading, readError, persist],
  );

  const retryConfig = useCallback(() => {
    if (readError !== null) {
      void refresh();
      return;
    }
    if (isLoading || saveStatus.state === 'saving') return;
    const intent = retryIntentRef.current;
    if (intent) void persist(intent.id, intent.config);
  }, [isLoading, readError, refresh, persist, saveStatus.state]);

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
