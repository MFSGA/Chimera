import { useClashLogs, type ClashLog } from '@chimera/interface';
import {
  createContext,
  useContext,
  useMemo,
  useState,
  type PropsWithChildren,
} from 'react';

const LogContext = createContext<{
  logs?: ClashLog[];
  filterText: string;
  setFilterText: (text: string) => void;
  logLevel: string;
  setLogLevel: (level: string) => void;
} | null>(null);

export const useLogContext = () => {
  const context = useContext(LogContext);

  if (!context) {
    throw new Error('useLogContext must be used within LogProvider');
  }

  return context;
};

export const LogProvider = ({ children }: PropsWithChildren) => {
  const [filterText, setFilterText] = useState('');
  const [logLevel, setLogLevel] = useState('all');
  const {
    query: { data },
  } = useClashLogs();

  const logs = useMemo(() => {
    return data?.filter((log: ClashLog) => {
      const matchesFilter =
        !filterText ||
        log.payload.toLowerCase().includes(filterText.toLowerCase());

      const matchesLevel = logLevel === 'all' ? true : log.type === logLevel;

      return matchesFilter && matchesLevel;
    });
  }, [data, filterText, logLevel]);

  return (
    <LogContext.Provider
      value={{
        logs,
        filterText,
        setFilterText,
        logLevel,
        setLogLevel,
      }}
    >
      {children}
    </LogContext.Provider>
  );
};
