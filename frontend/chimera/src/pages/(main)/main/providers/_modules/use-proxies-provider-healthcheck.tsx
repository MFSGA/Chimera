/** Provider-wide Clash healthcheck through the instance-bound Service/Local API. */
import {
  useHealthcheckClashProxiesProvider,
  type ClashProxiesProviderQueryItem,
} from '@chimera/interface';
import { useBlockTask } from '@/components/providers/block-task-provider';
import { formatError } from '@/utils';
import { message } from '@/utils/notification';

export const useProxiesProviderHealthcheck = (
  data: ClashProxiesProviderQueryItem,
) => {
  const healthcheck = useHealthcheckClashProxiesProvider();
  return useBlockTask(`healthcheck-proxies-provider-${data.name}`, async () => {
    try {
      await healthcheck.mutateAsync(data.name);
    } catch (error) {
      console.error('Failed to healthcheck proxies provider', error);
      await message(`Provider healthcheck failed: \n ${formatError(error)}`, {
        title: 'Error',
        kind: 'error',
      });
    }
  });
};
