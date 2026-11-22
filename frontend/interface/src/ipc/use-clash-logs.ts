import { useClashWSContext } from '../provider/clash-ws-provider';

export type ClashLog = {
  type: string;
  time?: string;
  payload: string;
};

export const useClashLogs = () => {
  const { logs, isLoading, error, clearHistory, retrySnapshot } =
    useClashWSContext();

  const clean = {
    mutateAsync: () => clearHistory('logs'),
  };

  return {
    data: logs,
    isLoading,
    error,
    clean,
    retry: retrySnapshot,
  };
};
