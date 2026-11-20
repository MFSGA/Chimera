import {
  isTauri,
  queries,
  TRANSPARENT_PROXY_STATUS_QUERY_KEY,
  unwrapQueryOptions,
  useClashSetting,
  useClashSettings,
  useSetting,
  useSystemService,
} from '@chimera/interface';
import { useQuery, useQueryClient } from '@tanstack/react-query';
import { useLockFn } from 'ahooks';
import { useState } from 'react';
import { OS } from '@/consts';
import { formatError } from '@/utils';
import { message } from '@/utils/notification';
import {
  canToggleTransparentProxy,
  transparentProxyNextPatch,
} from './transparent-proxy-policy';

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
  const transparentProxy = useClashSetting('transparent_proxy');
  const transparentActive =
    transparentProxy.value?.mode !== undefined &&
    transparentProxy.value.mode !== 'disabled';
  const blocked =
    (OS === 'windows' && enabling && !serviceReady) ||
    (enabling && transparentActive);

  const execute = useLockFn(async () => {
    if (blocked) {
      throw new Error(
        transparentActive
          ? 'Disable transparent proxy before enabling TUN.'
          : 'TUN on Windows requires Service Mode and a compatible running Chimera Service. The backend will verify and converge the core onto the service host.',
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

/** Ref-compatible Linux-only transparent capture shortcut. The backend owns
 * privileged rule installation; the UI only patches the typed persisted
 * config once all safety prerequisites have been confirmed. */
export const useTransparentProxyAction = () => {
  const supportedHost = isTauri() && OS === 'linux';
  const core = useSetting('core');
  const serviceMode = useSetting('enable_service_mode');
  const config = useClashSettings();
  const queryClient = useQueryClient();
  const supportedCore =
    core.value === 'mihomo' || core.value === 'mihomo-alpha';
  const visible = supportedHost && supportedCore;
  const statusOptions = queries.getTransparentProxyStatus();
  const status = useQuery({
    ...unwrapQueryOptions(statusOptions, statusOptions.queryFn!),
    enabled: visible && Boolean(serviceMode.value),
    refetchInterval: serviceMode.value ? 2000 : false,
  });
  const mode = config.query.data?.transparent_proxy?.mode ?? 'disabled';
  const isActive = mode !== 'disabled';
  const disabled =
    !canToggleTransparentProxy({
      supportedHost,
      supportedCore,
      serviceModeEnabled: Boolean(serviceMode.value),
      statusSupported: status.data?.supported === true && !status.isError,
      tunEnabled: Boolean(config.query.data?.enable_tun_mode),
      configReady: Boolean(config.query.data),
      mode,
    }) || config.upsert.isPending;
  const reason = !serviceMode.value
    ? 'Service Mode is required for transparent proxy'
    : config.query.data?.enable_tun_mode && !isActive
      ? 'Disable TUN before enabling transparent proxy'
      : status.isError
        ? 'Transparent proxy status unavailable'
        : status.isPending
          ? 'Checking transparent proxy support'
          : status.data && !status.data.supported
            ? 'Transparent proxy is unsupported by this host'
            : undefined;

  const execute = useLockFn(async () => {
    if (disabled || !config.query.data) return;
    try {
      await config.upsert.mutateAsync(
        transparentProxyNextPatch(mode, Boolean(config.query.data.tproxy_port)),
      );
      await queryClient.invalidateQueries({
        queryKey: [TRANSPARENT_PROXY_STATUS_QUERY_KEY],
      });
    } catch (error) {
      void message(formatError(error), { title: 'Error', kind: 'error' });
    }
  });

  return {
    visible,
    isActive,
    isPending: config.upsert.isPending,
    disabled,
    reason,
    status: status.data,
    mode,
    execute,
  };
};
