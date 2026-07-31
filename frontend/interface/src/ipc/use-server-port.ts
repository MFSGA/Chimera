import { useQuery } from '@tanstack/react-query';
import { unwrapResult } from '../utils';
import { commands } from './bindings';
import { SERVER_PORT_QUERY_KEY } from './consts';

/** Read the local HTTP server port used by cached resources. */
export const useServerPort = () => {
  return useQuery({
    queryKey: [SERVER_PORT_QUERY_KEY],
    queryFn: async () => unwrapResult(await commands.getServerPort()),
    staleTime: Number.POSITIVE_INFINITY,
  });
};
