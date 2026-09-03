import { useSetting, useSystemService } from '@chimera/interface';
import { useLockFn } from '@/hooks/use-lock-fn';
import * as m from '@/paraglide/messages';
import { formatError } from '@/utils';
import { message } from '@/utils/notification';

export function useSystemServiceMode() {
  const { query } = useSystemService();
  const serviceMode = useSetting('enable_service_mode');
  const isInstalled =
    query.data?.status === 'running' || query.data?.status === 'stopped';
  const isNotInstalled = query.data?.status === 'not_installed';

  const toggle = useLockFn(async () => {
    try {
      await serviceMode.upsert(!serviceMode.value);
    } catch (error) {
      message(
        `Activation Service Mode failed! \n Error: ${formatError(error)}`,
        {
          title: m.common_error(),
          kind: 'error',
        },
      );
    }
  });

  return {
    isInstalled,
    isNotInstalled,
    isPending: serviceMode.isPending,
    value: Boolean(serviceMode.value),
    toggle,
  };
}
