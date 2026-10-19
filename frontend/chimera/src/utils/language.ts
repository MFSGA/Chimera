import type { I18nLanguage } from '@chimera/interface';
import type { Locale } from '@/paraglide/runtime';

export const languageOptions = {
  en: 'English',
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

export const toI18nLanguage = (locale: Locale): I18nLanguage => {
  switch (locale) {
    case 'en':
      return 'en-US';
    case 'ru':
      return 'ru';
    case 'zh-cn':
      return 'zh-CN';
    case 'zh-tw':
      return 'zh-TW';
  }
};
