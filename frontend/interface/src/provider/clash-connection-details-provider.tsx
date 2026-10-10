import {
  createContext,
  useCallback,
  useContext,
  useEffect,
  useMemo,
  useState,
  type PropsWithChildren,
} from 'react';
import {
  commands,
  type ClashConnectionDetails,
  type ClashConnectionsConnectorState,
} from '../ipc/bindings';
import {
  createConnectionDetailsChannel,
  isTauri,
} from '../ipc/connection-details-channel';

type Registrar = () => () => void;
const RegistrarContext = createContext<Registrar | null>(null);

type ConnectionDetailsState = {
  frame: ClashConnectionDetails | null;
  status: 'idle' | 'connecting' | 'connected' | 'error';
  connectorState: ClashConnectionsConnectorState;
  error: unknown;
  retry: () => void;
};
const FrameContext = createContext<ConnectionDetailsState | null>(null);

/** Latest detail frame shared by every Main/Legacy Connections consumer. */
export const useClashConnectionDetails = () => {
  const register = useContext(RegistrarContext);
  const details = useContext(FrameContext);
  if (!register || !details) {
    throw new Error('useClashConnectionDetails requires its shared provider');
  }
  useEffect(() => register(), [register]);
  return {
    data: details.frame,
    isLoading: details.frame === null,
    status: details.status,
    connectorState: details.connectorState,
    error: details.error,
    retry: details.retry,
  };
};

/** Keeps an exiting/animated page from following new high-frequency frames. */
export function ClashConnectionDetailsFreezeBoundary({
  frozen,
  children,
}: PropsWithChildren<{ frozen: boolean }>) {
  const live = useContext(FrameContext);
  const [captured, setCaptured] = useState<ConnectionDetailsState | null>(null);
  if (frozen && captured === null) {
    setCaptured(live);
  } else if (!frozen && captured !== null) {
    setCaptured(null);
  }
  return (
    <FrameContext.Provider value={frozen ? (captured ?? live) : live}>
      {children}
    </FrameContext.Provider>
  );
}

export function ClashConnectionDetailsProvider({
  connectorState,
  children,
}: PropsWithChildren<{ connectorState: ClashConnectionsConnectorState }>) {
  const [subscribers, setSubscribers] = useState(0);
  const [frame, setFrame] = useState<ClashConnectionDetails | null>(null);
  const [status, setStatus] =
    useState<ConnectionDetailsState['status']>('idle');
  const [error, setError] = useState<unknown>(null);
  const [retries, setRetries] = useState(0);

  const register = useCallback<Registrar>(() => {
    setSubscribers((value) => value + 1);
    return () => setSubscribers((value) => value - 1);
  }, []);
  const retry = useCallback(() => setRetries((value) => value + 1), []);

  useEffect(() => {
    if (connectorState !== 'connected') {
      setFrame(null);
      setStatus('idle');
      setError(null);
    }
  }, [connectorState]);

  const hasSubscribers = subscribers > 0;
  useEffect(() => {
    if (!hasSubscribers || connectorState !== 'connected') {
      return;
    }

    setFrame(null);
    setStatus('connecting');
    setError(null);
    if (!isTauri()) {
      setError(
        new Error('Connection details require the native Tauri transport'),
      );
      setStatus('error');
      return;
    }

    let disposed = false;
    let subscriptionId: number | undefined;
    const channel = createConnectionDetailsChannel<ClashConnectionDetails>(
      (value) => {
        if (disposed) return;
        setFrame(value);
        setStatus('connected');
        setError(null);
      },
    );
    const unsubscribe = (id: number) => {
      commands
        .unsubscribeClashConnectionDetails(id)
        .then((result) => {
          if (result.status === 'error') {
            console.error(
              'Failed to release connection details subscription',
              result.error,
            );
          }
        })
        .catch((reason: unknown) =>
          console.error('Failed to release detail subscription', reason),
        );
    };

    commands
      .subscribeClashConnectionDetails(channel)
      .then((result) => {
        if (disposed) {
          if (result.status === 'ok') unsubscribe(result.data);
          return;
        }
        if (result.status === 'error') {
          setError(result.error);
          setStatus('error');
          return;
        }
        subscriptionId = result.data;
      })
      .catch((reason: unknown) => {
        if (!disposed) {
          setError(reason);
          setStatus('error');
        }
      });

    return () => {
      disposed = true;
      setFrame(null);
      setStatus('idle');
      setError(null);
      if (subscriptionId !== undefined) unsubscribe(subscriptionId);
    };
  }, [connectorState, hasSubscribers, retries]);

  const value = useMemo(
    () => ({ frame, status, connectorState, error, retry }),
    [frame, status, connectorState, error, retry],
  );

  return (
    <RegistrarContext.Provider value={register}>
      <FrameContext.Provider value={value}>{children}</FrameContext.Provider>
    </RegistrarContext.Provider>
  );
}
