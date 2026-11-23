/** Provider-wide Clash healthcheck through the instance-bound Service/Local API. */
import {
  useHealthcheckClashProxiesProvider,
  type ClashProxiesProviderQueryItem,
} from '@chimera/interface';
import { useBlockTask } from '@/components/providers/block-task-provider';
import * as m from '@/paraglide/messages';
import { message } from '@/utils/notification';
import { runProviderAction } from './provider-action';

export const useProxiesProviderHealthcheck = (
  data: ClashProxiesProviderQueryItem,
) => {
  const healthcheck = useHealthcheckClashProxiesProvider();
  return useBlockTask(`healthcheck-proxies-provider-${data.name}`, () =>
    runProviderAction(
      () => healthcheck.mutateAsync(data.name),
      async (error) => {
        console.error('Failed to healthcheck proxies provider', error);
        await message(
          m.providers_healthcheck_failed_message({ name: data.name }),
          {
            title: 'Error',
            kind: 'error',
          },
        );
      },
    ),
  );
};
