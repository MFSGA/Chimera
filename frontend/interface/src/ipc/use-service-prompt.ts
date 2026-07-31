import { useQuery } from '@tanstack/react-query';
import { unwrapResult } from '../utils';
import { commands } from './bindings';
import { SERVICE_PROMPT_QUERY_KEY } from './consts';

/** Load the platform-specific manual service installation command. */
export const useServicePrompt = (enabled = true) => {
  return useQuery({
    queryKey: [SERVICE_PROMPT_QUERY_KEY],
    queryFn: async () => unwrapResult(await commands.getServiceInstallPrompt()),
    enabled,
  });
};
