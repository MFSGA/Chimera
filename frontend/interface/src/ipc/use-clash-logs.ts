import { useMemo } from 'react';
import { useCoreLogs } from './use-core-logs';

export type ClashLog = { type: string; time?: string; payload: string };
/** Legacy UI projection only. No separate log capture or backend cache. */
export const useClashLogs = () => {
  const logs = useCoreLogs();
  const data = useMemo(
    () =>
      logs.data.map(({ record }) => ({
        type: record.type,
        time: record.time ?? undefined,
        payload: record.payload,
      })),
    [logs.data],
  );
  return { ...logs, data };
};
