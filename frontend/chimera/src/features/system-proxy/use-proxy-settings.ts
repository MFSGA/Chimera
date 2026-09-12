import {
  useCoreStatus,
  useSetting,
  useSystemService,
} from '@chimera/interface';
import { useLockFn } from 'ahooks';
import { useState } from 'react';
import { OS } from '@/consts';

type ProxySettingKey = 'enable_system_proxy' | 'enable_tun_mode';

/** Expose a serialized application action for toggling a proxy setting. */
const useProxySetting = (key: ProxySettingKey) => {
  const setting = useSetting(key);
  const [isPending, setIsPending] = useState(false);

  const execute = useLockFn(async () => {
    setIsPending(true);
    try {
      await setting.upsert(!setting.value);
    } finally {
      setIsPending(false);
    }
  });

  return {
    execute,
    isPending,
    isActive: Boolean(setting.value),
  };
};

export const useSystemProxyAction = () =>
  useProxySetting('enable_system_proxy');

export const useTunModeAction = () => {
  const action = useProxySetting('enable_tun_mode');
  const serviceMode = useSetting('enable_service_mode');
  const service = useSystemService();
  const core = useCoreStatus();
  const enabling = !action.isActive;
  const serviceReady =
    serviceMode.value === true &&
    service.query.data?.status === 'running' &&
    service.query.data.compat.kind === 'compatible' &&
    core.data?.type === 'service';
  const blocked = OS === 'windows' && enabling && !serviceReady;

  const execute = useLockFn(async () => {
    if (blocked) {
      throw new Error(
        'TUN on Windows requires a compatible running Chimera Service, Service Mode enabled, and the core running on the service host.',
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
