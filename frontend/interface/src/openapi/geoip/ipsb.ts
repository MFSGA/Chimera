import useSWR, { type SWRConfiguration } from 'swr';
import { getIpsbASN } from '../../service';

export const useIPSB = (config?: SWRConfiguration) => {
  return useSWR('https://api.ip.sb/geoip', () => getIpsbASN(), config);
};
