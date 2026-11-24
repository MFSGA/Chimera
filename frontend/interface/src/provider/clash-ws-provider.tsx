import {
  createContext,
  useCallback,
  useContext,
  useEffect,
  useRef,
  useState,
  type PropsWithChildren,
} from 'react';
import {
  commands,
  events,
  type ClashWsEvent,
  type ClashWsKind,
  type ClashWsSnapshot,
} from '../ipc/bindings';
import { unwrapResult } from '../utils';
import { applyClashWsEvent } from './clash-ws-state';

const ClashWSContext = createContext<{
  connections: ClashWsSnapshot['connections'];
  traffic: ClashWsSnapshot['traffic'];
  memory: ClashWsSnapshot['memory'];
  isLoading: boolean;
  connectorState: ClashWsSnapshot['state'];
  error: unknown;
  clearHistory: (kind: ClashWsKind) => Promise<void>;
  retrySnapshot: () => Promise<void>;
} | null>(null);
export const useClashWSContext = () => {
  const context = useContext(ClashWSContext);
  if (!context) {
    throw new Error('useClashWSContext must be used in a ClashWSProvider');
  }
  return context;
};
export const ClashWSProvider = ({ children }: PropsWithChildren) => {
  const [snapshot, setSnapshot] = useState<ClashWsSnapshot>();
  const [isLoading, setIsLoading] = useState(true);
  const [error, setError] = useState<unknown>(null);
  const retry = useRef<() => Promise<void>>(async () => {});
  useEffect(() => {
    let disposed = false;
    let current: ClashWsSnapshot | undefined;
    let syncing = false;
    let pending: ClashWsEvent[] = [];
    let unlisten: (() => void) | undefined;
    let subscribing = false;
    const resync = async () => {
      if (syncing || disposed || !unlisten) return;
      syncing = true;
      try {
        do {
          const result = unwrapResult(await commands.getClashWsSnapshot());
          if (disposed) return;
          if (!current || result.sequence >= current.sequence) current = result;
          const buffered = pending;
          pending = [];
          let gap = false;
          for (const event of buffered) {
            const next = applyClashWsEvent(current, event);
            if (!next) {
              gap = true;
              break;
            }
            current = next;
          }
          if (!gap) break;
        } while (!disposed);
        setSnapshot(current);
        setError(null);
      } catch (cause) {
        if (!disposed) setError(cause);
      } finally {
        syncing = false;
        if (!disposed) setIsLoading(false);
      }
    };
    const subscribe = async () => {
      if (disposed || subscribing || unlisten) return;
      subscribing = true;
      try {
        const stop = await events.clashWsEvent.listen(({ payload }) => {
          if (disposed) return;
          if (syncing || !current) {
            pending = [...pending, payload].slice(-256);
            void resync();
            return;
          }
          const next = applyClashWsEvent(current, payload);
          if (!next) {
            pending = [payload];
            void resync();
            return;
          }
          current = next;
          if (document.visibilityState !== 'hidden') setSnapshot(next);
        });
        if (disposed) {
          stop();
          return;
        }
        unlisten = stop;
        await resync();
      } catch (cause) {
        if (!disposed) {
          setError(cause);
          setIsLoading(false);
        }
      } finally {
        subscribing = false;
      }
    };
    retry.current = async () => {
      setIsLoading(true);
      if (unlisten) await resync();
      else await subscribe();
    };
    const resume = () => {
      if (document.visibilityState !== 'hidden') {
        if (current) setSnapshot(current);
        void retry.current();
      }
    };
    document.addEventListener('visibilitychange', resume);
    window.addEventListener('focus', resume);
    void subscribe();
    return () => {
      disposed = true;
      unlisten?.();
      document.removeEventListener('visibilitychange', resume);
      window.removeEventListener('focus', resume);
      retry.current = async () => {};
    };
  }, []);
  const clearHistory = useCallback(async (kind: ClashWsKind) => {
    unwrapResult(await commands.clearClashWsHistory(kind));
    await retry.current();
  }, []);
  const retrySnapshot = useCallback(() => retry.current(), []);
  return (
    <ClashWSContext.Provider
      value={{
        connections: snapshot?.connections ?? [],
        traffic: snapshot?.traffic ?? [],
        memory: snapshot?.memory ?? [],
        connectorState: snapshot?.state ?? 'disconnected',
        isLoading,
        error,
        clearHistory,
        retrySnapshot,
      }}
    >
      {children}
    </ClashWSContext.Provider>
  );
};
