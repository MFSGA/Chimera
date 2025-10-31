import { isAppImage } from '@chimera/interface';
import useSWR, { SWRConfiguration } from 'swr';

export const useIsAppImage = (config?: Partial<SWRConfiguration>) => {
  return useSWR<boolean>('/api/is_appimage', isAppImage, {
    ...(config || {}),
    revalidateOnFocus: false,
    revalidateOnReconnect: false,
    refreshInterval: 0,
  });
};
