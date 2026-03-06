import { useUpdateEffect } from 'ahooks';
import { useQuery } from '@tanstack/react-query';

import { commands } from './bindings';
import { unwrapResult } from '../utils';
import { CHIMERA_SYSTEM_PROXY_QUERY_KEY } from './consts';
import { useSetting } from './use-settings';

export const useSystemProxy = () => {
  const query = useQuery({
    queryKey: [CHIMERA_SYSTEM_PROXY_QUERY_KEY],
    queryFn: async () => unwrapResult(await commands.getSysProxy()),
    refetchInterval: 5000,
    refetchIntervalInBackground: true,
  });

  const { value } = useSetting('enable_system_proxy');

  useUpdateEffect(() => {
    query.refetch();
  }, [value]);

  return query;
};
