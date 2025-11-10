import { useSetting } from '@chimera/interface';
import { locale } from 'dayjs';
import { changeLanguage } from 'i18next';
import { useEffect } from 'react';

export const LocalesProvider = () => {
  const { value } = useSetting('language');

  useEffect(() => {
    if (value) {
      locale(value === 'zh' ? 'zh-cn' : value);

      changeLanguage(value);
    }
  }, [value]);

  return null;
};

export default LocalesProvider;
