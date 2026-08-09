import {
  createContext,
  useContext,
  useState,
  type PropsWithChildren,
} from 'react';

const DebugContext = createContext<{
  advancedTools: boolean;
  setAdvancedTools: (value: boolean) => void;
} | null>(null);

export const useDebugContext = () => {
  const context = useContext(DebugContext);
  if (!context) {
    throw new Error('useDebugContext must be used within DebugProvider');
  }
  return context;
};

export default function DebugProvider({ children }: PropsWithChildren) {
  const [advancedTools, setAdvancedTools] = useState(import.meta.env.DEV);

  return (
    <DebugContext.Provider value={{ advancedTools, setAdvancedTools }}>
      {children}
    </DebugContext.Provider>
  );
}
