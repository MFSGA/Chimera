import {
  useClashSetting,
  useSetting,
  useSystemService,
} from '@chimera/interface';
import { useLockFn } from 'ahooks';
import { useState } from 'react';
import { OS } from '@/consts';

/** Expose a serialized action for toggling a proxy-related setting. */
const useProxySetting = (
  value: boolean | undefined,
  upsert: (value: boolean) => Promise<unknown>,
) => {
  const [isPending, setIsPending] = useState(false);

  const execute = useLockFn(async () => {
    setIsPending(true);
    try {
      await upsert(!value);
    } finally {
      setIsPending(false);
    }
  });

  return {
    execute,
    isPending,
    isActive: Boolean(value),
  };
};

export const useSystemProxyAction = () => {
  const setting = useSetting('enable_system_proxy');
  return useProxySetting(setting.value, setting.upsert);
};

export const useTunModeAction = () => {
  const setting = useClashSetting('enable_tun_mode');
  const action = useProxySetting(setting.value, setting.upsert);
  const serviceMode = useSetting('enable_service_mode');
  const service = useSystemService();
  const enabling = !action.isActive;
  const serviceReady =
    serviceMode.value === true &&
    service.query.data?.status === 'running' &&
    service.query.data.compat.kind === 'compatible';
  const blocked = OS === 'windows' && enabling && !serviceReady;

  const execute = useLockFn(async () => {
    if (blocked) {
      throw new Error(
        'TUN on Windows requires Service Mode and a compatible running Chimera Service. The backend will verify and converge the core onto the service host.',
      );
    }
    await action.execute();
  });

  return {
    ...action,
    execute,
    blocked,
    serviceReady,
  };
};
