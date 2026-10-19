import type { I18nLanguage_Serialize } from '@chimera/interface';
import {
  defineCustomClientStrategy,
  locales,
  type Locale,
} from '@/paraglide/runtime';

export type Language = (typeof locales)[number];

export const LANGUAGE_STORAGE_KEY = 'paraglide-language-cache';

const CACHED_LANGUAGE_STORAGE_KEY = btoa(LANGUAGE_STORAGE_KEY);

const LEGACY_LANGUAGE_ALIASES: Record<string, Language> = {
  'en-us': 'en',
  'ja-jp': 'ja',
};

export const normalizeLanguage = (value: unknown): Language | null => {
  if (typeof value !== 'string') return null;

  const normalized = value.toLowerCase();
  return (
    locales.find((locale) => locale === normalized) ??
    LEGACY_LANGUAGE_ALIASES[normalized] ??
    null
  );
};

export const setCachedLanguage = (language: Language) => {
  localStorage.setItem(CACHED_LANGUAGE_STORAGE_KEY, language);
};

export const getCachedLanguage = (): Language | null =>
  normalizeLanguage(localStorage.getItem(CACHED_LANGUAGE_STORAGE_KEY));

defineCustomClientStrategy('custom-extension', {
  getLocale: () => {
    const language = getCachedLanguage() ?? undefined;
    if (language && typeof document !== 'undefined') {
      document.documentElement.lang = language;
    }
    return language;
  },
  setLocale: (language) => {
    setCachedLanguage(language as Language);
    if (typeof document !== 'undefined') {
      document.documentElement.lang = String(language).toLowerCase();
    }
  },
});

export const languageOptions: Record<Language, string> = {
  en: 'English',
  ja: '日本語',
  ru: 'Русский',
  'zh-cn': '简体中文',
  'zh-tw': '繁體中文',
};

export const languageQuirks: {
  [key: string]: {
    drawer: {
      minWidth: number;
      itemClassNames?: string;
    };
  };
} = {
  en: {
    drawer: {
      minWidth: 240,
    },
  },
  ja: {
    drawer: {
      minWidth: 180,
    },
  },
  ru: {
    drawer: {
      minWidth: 240,
    },
  },
  'zh-cn': {
    drawer: {
      minWidth: 180,
    },
  },
  'zh-tw': {
    drawer: {
      minWidth: 180,
    },
  },
};

export const toI18nLanguage = (locale: Locale): I18nLanguage_Serialize => {
  switch (locale) {
    case 'en':
      return 'en';
    case 'ja':
      return 'ja';
    case 'ru':
      return 'ru';
    case 'zh-cn':
      return 'zh-cn';
    case 'zh-tw':
      return 'zh-tw';
  }
};
