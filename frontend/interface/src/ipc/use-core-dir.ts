import { useQuery } from '@tanstack/react-query';
import { unwrapResult } from '../utils';
import { commands } from './bindings';
import { CORE_DIR_QUERY_KEY } from './consts';

/** Read the directory that contains the managed core binaries. */
export const useCoreDir = () => {
  return useQuery({
    queryKey: [CORE_DIR_QUERY_KEY],
    queryFn: async () => unwrapResult(await commands.getCoreDir()),
    staleTime: Number.POSITIVE_INFINITY,
  });
};
