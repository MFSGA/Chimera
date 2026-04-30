import { useSetting } from '@chimera/interface';
import { useLockFn } from 'ahooks';
import { useState } from 'react';

const useProxySetting = (key: 'enable_system_proxy' | 'enable_tun_mode') => {
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

export const useSystemProxy = () => {
  return useProxySetting('enable_system_proxy');
};

export const useTunMode = () => {
  return useProxySetting('enable_tun_mode');
};
