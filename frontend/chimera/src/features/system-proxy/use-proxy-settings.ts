import { useSetting } from '@chimera/interface';
import { useLockFn } from 'ahooks';
import { useState } from 'react';

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

export const useTunModeAction = () => useProxySetting('enable_tun_mode');
