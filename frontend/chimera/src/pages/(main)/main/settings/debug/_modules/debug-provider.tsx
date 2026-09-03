import { createContext, use, useState, type PropsWithChildren } from 'react';

const isDev = import.meta.env.DEV;

const DebugContext = createContext<{
  advanceTools: boolean;
  setAdvanceTools: (value: boolean) => void;
} | null>(null);

export const useDebugContext = () => {
  const context = use(DebugContext);

  if (!context) {
    throw new Error('useDebugContext must be used within a DebugProvider');
  }

  return context;
};

export default function DebugProvider({ children }: PropsWithChildren) {
  const [advanceTools, setAdvanceTools] = useState(isDev);

  return (
    <DebugContext.Provider
      value={{
        advanceTools,
        setAdvanceTools,
      }}
    >
      {children}
    </DebugContext.Provider>
  );
}
