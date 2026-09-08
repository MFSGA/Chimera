import { useSetting } from '@chimera/interface';
import { locale } from 'dayjs';
import { createContext, PropsWithChildren, useContext, useEffect } from 'react';
import { useLockFn } from '@/hooks/use-lock-fn';
import { getLocale, Locale, locales, setLocale } from '@/paraglide/runtime';

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

const normalizeConfiguredLocale = (
  value?: string | null,
): Locale | undefined => {
  if (!value) return undefined;

  const normalized = value.toLowerCase();
  if (normalized === 'en-us') return 'en';
  return locales.includes(normalized as Locale)
    ? (normalized as Locale)
    : undefined;
};

export const LanguageProvider = ({ children }: PropsWithChildren) => {
  const language = useSetting('language');
  const configuredLocale = normalizeConfiguredLocale(language.value);

  const setLanguage = useLockFn(async (value: Locale) => {
    await language.upsert(value);
    setLocale(value);
  });

  // Keep Paraglide and dayjs aligned when another WebView changes the setting.
  useEffect(() => {
    if (!configuredLocale) return;

    locale(configuredLocale);
    if (getLocale() !== configuredLocale) {
      setLocale(configuredLocale);
    }
  }, [configuredLocale]);

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
