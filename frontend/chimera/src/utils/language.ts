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
