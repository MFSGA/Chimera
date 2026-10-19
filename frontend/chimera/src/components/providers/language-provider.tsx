import { useSettings } from '@chimera/interface';
import { locale } from 'dayjs';
import {
  createContext,
  PropsWithChildren,
  useContext,
  useEffect,
  useState,
} from 'react';
import { useLockFn } from '@/hooks/use-lock-fn';
import { getLocale, Locale, setLocale } from '@/paraglide/runtime';
import {
  getCachedLanguage,
  normalizeLanguage,
  toI18nLanguage,
} from '@/utils/language';

const LanguageContext = createContext<{
  language?: Locale;
  setLanguage: (value: Locale) => Promise<void>;
} | null>(null);

export const useLanguage = () => {
  const context = useContext(LanguageContext);

  if (!context) {
    throw new Error('useLanguage must be used within a LanguageProvider');
  }

  return context;
};

/**
 * Applies the language resolved by the backend, which uses the system locale
 * on first launch and the user's saved choice afterwards.
 */
export const LanguageProvider = ({ children }: PropsWithChildren) => {
  const { query, upsert } = useSettings();
  const persisted = normalizeLanguage(query.data?.language);
  const settled = query.isSuccess || query.isError;
  const [cached] = useState(getCachedLanguage);
  const [applied, setApplied] = useState(cached !== null);

  useEffect(() => {
    if (applied || !settled) return;

    if (persisted && persisted !== cached) {
      locale(persisted);
      setLocale(persisted, { reload: false });
    }

    setApplied(true);
  }, [applied, settled, persisted, cached]);

  useEffect(() => {
    if (!applied || !persisted) return;

    locale(persisted);
    if (persisted === getLocale()) return;
    setLocale(persisted);
  }, [applied, persisted]);

  const setLanguage = useLockFn(async (value: Locale) => {
    await upsert.mutateAsync({ language: toI18nLanguage(value) });
    locale(value);
    setLocale(value);
  });

  if (!applied) {
    return null;
  }

  return (
    <LanguageContext.Provider
      value={{
        language: getLocale(),
        setLanguage,
      }}
    >
      {children}
    </LanguageContext.Provider>
  );
};
