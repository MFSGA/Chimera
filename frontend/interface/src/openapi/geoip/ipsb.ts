import { useQuery } from '@tanstack/react-query';
import { getIpsbASN } from '../../service';

const IPSB_QUERY_KEY = 'ipsb-geoip';
const IPSB_REFRESH_INTERVAL = 180_000;

/** Query public IP and ASN information through the Tauri backend. */
export const useIPSB = () => {
  return useQuery({
    queryKey: [IPSB_QUERY_KEY],
    queryFn: getIpsbASN,
    refetchInterval: IPSB_REFRESH_INTERVAL,
    refetchOnWindowFocus: false,
  });
};
