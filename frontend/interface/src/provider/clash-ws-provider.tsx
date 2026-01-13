import { useQueryClient } from '@tanstack/react-query';
import { useUpdateEffect } from 'ahooks';
import dayjs from 'dayjs';
import {
  createContext,
  useCallback,
  useContext,
  useEffect,
  useMemo,
  useRef,
  useState,
  type PropsWithChildren,
} from 'react';
import {
  CLASH_CONNECTIONS_QUERY_KEY,
  CLASH_LOGS_QUERY_KEY,
  CLASH_MEMORY_QUERY_KEY,
  CLASH_TRAAFFIC_QUERY_KEY,
  MAX_CONNECTIONS_HISTORY,
  MAX_LOGS_HISTORY,
  MAX_MEMORY_HISTORY,
  MAX_TRAFFIC_HISTORY,
} from '../ipc/consts';
import type { ClashConnection } from '../ipc/use-clash-connections';
import type { ClashLog } from '../ipc/use-clash-logs';
import type { ClashMemory } from '../ipc/use-clash-memory';
import type { ClashTraffic } from '../ipc/use-clash-traffic';
import { useClashWebSocket } from '../ipc/use-clash-web-socket';

const BATCH_INTERVAL_MS = 1000;

// Utility functions for localStorage persistence
const createPersistedState = (key: string, defaultValue: boolean) => {
  const getStoredValue = (): boolean => {
    try {
      const item = localStorage.getItem(key);
      return item ? JSON.parse(item) : defaultValue;
    } catch {
      return defaultValue;
    }
  };

  const setStoredValue = (value: boolean) => {
    try {
      localStorage.setItem(key, JSON.stringify(value));
    } catch {
      // Ignore storage errors
    }
  };

  return { getStoredValue, setStoredValue };
};

const appendHistory = <T,>(
  current: T[] | undefined,
  incoming: T[],
  maxHistory: number,
) => {
  const merged = [...(current || []), ...incoming];

  if (merged.length > maxHistory) {
    return merged.slice(-maxHistory);
  }

  return merged;
};

const parseJson = <T,>(data: unknown): T | null => {
  if (typeof data !== 'string') {
    return null;
  }

  try {
    return JSON.parse(data) as T;
  } catch {
    return null;
  }
};

const parseConnections = (data: unknown) => {
  return parseJson<ClashConnection>(data);
};

const parseMemory = (data: unknown) => {
  return parseJson<ClashMemory>(data);
};

const parseTraffic = (data: unknown) => {
  return parseJson<ClashTraffic>(data);
};

const parseLogs = (data: unknown) => {
  const parsed = parseJson<ClashLog>(data);
  if (!parsed) {
    return null;
  }

  return {
    ...parsed,
    time: dayjs().format('HH:mm:ss'),
  };
};

type BufferedQueryUpdaterOptions<T> = {
  enabled: boolean;
  latestData: unknown;
  parse: (data: unknown) => T | null;
  queryKey: string;
  maxHistory: number;
  intervalMs?: number;
};

const useBufferedQueryUpdater = <T,>({
  enabled,
  latestData,
  parse,
  queryKey,
  maxHistory,
  intervalMs = BATCH_INTERVAL_MS,
}: BufferedQueryUpdaterOptions<T>) => {
  const queryClient = useQueryClient();
  const queueRef = useRef<T[]>([]);
  const lastDataRef = useRef<unknown>(undefined);

  // 1) enqueue parsed data on message updates, 2) flush queue on interval.
  useUpdateEffect(() => {
    if (latestData === lastDataRef.current) {
      return;
    }

    lastDataRef.current = latestData;

    if (!enabled) {
      return;
    }

    const parsed = parse(latestData);
    if (!parsed) {
      return;
    }

    queueRef.current.push(parsed);
  }, [enabled, latestData, parse]);

  useEffect(() => {
    if (!enabled) {
      queueRef.current = [];
      return;
    }

    const intervalId = setInterval(() => {
      const queued = queueRef.current;
      if (queued.length === 0) {
        return;
      }

      queueRef.current = [];
      queryClient.setQueryData([queryKey], (current: T[] | undefined) => {
        return appendHistory(current, queued, maxHistory);
      });
    }, intervalMs);

    return () => clearInterval(intervalId);
  }, [enabled, intervalMs, maxHistory, queryClient, queryKey]);
};

type ClashWSUpdaterProps = {
  recordLogs: boolean;
  recordTraffic: boolean;
  recordMemory: boolean;
  recordConnections: boolean;
};

const ClashWSUpdater = ({
  recordLogs,
  recordTraffic,
  recordMemory,
  recordConnections,
}: ClashWSUpdaterProps) => {
  const { connectionsWS, memoryWS, trafficWS, logsWS } = useClashWebSocket();

  // Keep websocket-driven rerenders scoped to this component.
  useBufferedQueryUpdater({
    enabled: recordConnections,
    latestData: connectionsWS.latestMessage?.data,
    parse: parseConnections,
    queryKey: CLASH_CONNECTIONS_QUERY_KEY,
    maxHistory: MAX_CONNECTIONS_HISTORY,
  });

  useBufferedQueryUpdater({
    enabled: recordMemory,
    latestData: memoryWS.latestMessage?.data,
    parse: parseMemory,
    queryKey: CLASH_MEMORY_QUERY_KEY,
    maxHistory: MAX_MEMORY_HISTORY,
  });

  useBufferedQueryUpdater({
    enabled: recordTraffic,
    latestData: trafficWS.latestMessage?.data,
    parse: parseTraffic,
    queryKey: CLASH_TRAAFFIC_QUERY_KEY,
    maxHistory: MAX_TRAFFIC_HISTORY,
  });

  useBufferedQueryUpdater({
    enabled: recordLogs,
    latestData: logsWS.latestMessage?.data,
    parse: parseLogs,
    queryKey: CLASH_LOGS_QUERY_KEY,
    maxHistory: MAX_LOGS_HISTORY,
  });

  return null;
};

const ClashWSContext = createContext<{
  recordLogs: boolean;
  setRecordLogs: (value: boolean) => void;
  recordTraffic: boolean;
  setRecordTraffic: (value: boolean) => void;
  recordMemory: boolean;
  setRecordMemory: (value: boolean) => void;
  recordConnections: boolean;
  setRecordConnections: (value: boolean) => void;
} | null>(null);

export const useClashWSContext = () => {
  const context = useContext(ClashWSContext);

  if (!context) {
    throw new Error('useClashWSContext must be used in a ClashWSProvider');
  }

  return context;
};

export const ClashWSProvider = ({ children }: PropsWithChildren) => {
  // Create persisted state handlers
  const logsStorage = useMemo(
    () => createPersistedState('clash-ws-record-logs', true),
    [],
  );
  const trafficStorage = useMemo(
    () => createPersistedState('clash-ws-record-traffic', true),
    [],
  );
  const memoryStorage = useMemo(
    () => createPersistedState('clash-ws-record-memory', true),
    [],
  );
  const connectionsStorage = useMemo(
    () => createPersistedState('clash-ws-record-connections', true),
    [],
  );

  // Initialize states with persisted values
  const [recordLogs, setRecordLogsState] = useState(logsStorage.getStoredValue);
  const [recordTraffic, setRecordTrafficState] = useState(
    trafficStorage.getStoredValue,
  );
  const [recordMemory, setRecordMemoryState] = useState(
    memoryStorage.getStoredValue,
  );
  const [recordConnections, setRecordConnectionsState] = useState(
    connectionsStorage.getStoredValue,
  );

  // Wrapped setters that also persist to localStorage.
  const setRecordLogs = useCallback(
    (value: boolean) => {
      setRecordLogsState(value);
      logsStorage.setStoredValue(value);
    },
    [logsStorage],
  );

  const setRecordTraffic = useCallback(
    (value: boolean) => {
      setRecordTrafficState(value);
      trafficStorage.setStoredValue(value);
    },
    [trafficStorage],
  );

  const setRecordMemory = useCallback(
    (value: boolean) => {
      setRecordMemoryState(value);
      memoryStorage.setStoredValue(value);
    },
    [memoryStorage],
  );

  const setRecordConnections = useCallback(
    (value: boolean) => {
      setRecordConnectionsState(value);
      connectionsStorage.setStoredValue(value);
    },
    [connectionsStorage],
  );

  const contextValue = useMemo(
    () => ({
      recordLogs,
      setRecordLogs,
      recordTraffic,
      setRecordTraffic,
      recordMemory,
      setRecordMemory,
      recordConnections,
      setRecordConnections,
    }),
    [
      recordLogs,
      setRecordLogs,
      recordTraffic,
      setRecordTraffic,
      recordMemory,
      setRecordMemory,
      recordConnections,
      setRecordConnections,
    ],
  );

  return (
    <ClashWSContext.Provider value={contextValue}>
      <ClashWSUpdater
        recordLogs={recordLogs}
        recordTraffic={recordTraffic}
        recordMemory={recordMemory}
        recordConnections={recordConnections}
      />
      {children}
    </ClashWSContext.Provider>
  );
};
