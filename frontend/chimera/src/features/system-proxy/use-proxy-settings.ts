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
import { useEffect, useState } from 'react';
import { OS } from '@/consts';
import * as m from '@/paraglide/messages';
import { formatError } from '@/utils';
import { message } from '@/utils/notification';
import { proxyToggleGate } from './proxy-toggle-gate';
import {
  canToggleTransparentProxy,
  isTransparentProxyApplied,
  transparentProxyNextPatch,
} from './transparent-proxy-policy';

/** Expose a safe, serialized action for a persisted proxy setting.
 * Writes are shared across Settings, Dashboard and Legacy UI instances. */
const useProxySetting = (
  key: 'system' | 'tun',
  value: boolean | undefined,
  upsert: (value: boolean) => Promise<unknown>,
  refetch: () => Promise<unknown>,
) => {
  const [isPending, setIsPending] = useState(false);

  const execute = useLockFn(async () => {
    if (value === undefined) return;
    setIsPending(true);
    try {
      await proxyToggleGate.run(key, value, upsert, refetch);
    } finally {
      setIsPending(false);
    }
  });

  return {
    execute,
    isPending,
    isReady: value !== undefined,
    isActive: Boolean(value),
  };
};

export const useSystemProxyAction = () => {
  const setting = useSetting('enable_system_proxy');
  return useProxySetting(
    'system',
    setting.value,
    setting.upsert,
    setting.refetch,
  );
};

export const useTunModeAction = () => {
  const setting = useClashSetting('enable_tun_mode');
  const action = useProxySetting(
    'tun',
    setting.value,
    setting.upsert,
    setting.refetch,
  );
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
    !action.isReady ||
    (OS === 'windows' && enabling && !serviceReady) ||
    (enabling && transparentActive);
  const blockedReason =
    transparentActive && enabling
      ? m.error_transparent_proxy_tun_conflict()
      : OS === 'windows' && enabling && !serviceReady
        ? m.settings_system_proxy_service_mode_disabled_tooltip()
        : undefined;

  const execute = useLockFn(async () => {
    if (blocked) {
      if (blockedReason) throw new Error(blockedReason);
      return;
    }
    await action.execute();
  });

  return {
    ...action,
    execute,
    blocked,
    blockedReason,
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
  // An old status snapshot belongs to the previous host/core/service state.
  // Invalidate the shared query when any of those gates change.
  useEffect(() => {
    if (!visible || !serviceMode.value) return;
    void queryClient.invalidateQueries({
      queryKey: [TRANSPARENT_PROXY_STATUS_QUERY_KEY],
    });
  }, [core.value, serviceMode.value, visible, queryClient]);
  const [isSyncing, setIsSyncing] = useState(false);

  const mode = config.query.data?.transparent_proxy?.mode ?? 'disabled';
  const isActive = mode !== 'disabled';
  const serviceModeEnabled = serviceMode.value === true;
  const applied = isTransparentProxyApplied({
    visible,
    serviceModeEnabled,
    configuredMode: mode,
    status: status.isError ? null : status.data,
    statusFresh: status.dataUpdatedAt >= config.query.dataUpdatedAt,
    isSyncing,
  });
  const enabledStatusReady =
    !status.isError &&
    !status.isPending &&
    !status.isFetching &&
    status.data?.supported === true &&
    !status.data.error;
  const isPending = config.upsert.isPending || isSyncing;
  const disabled =
    !canToggleTransparentProxy({
      supportedHost,
      supportedCore,
      serviceModeEnabled,
      statusSupported: enabledStatusReady,
      tunEnabled: Boolean(config.query.data?.enable_tun_mode),
      configReady: Boolean(config.query.data),
      mode,
    }) || isPending;
  const reason = !serviceModeEnabled
    ? m.settings_clash_transparent_proxy_service_mode_required()
    : config.query.data?.enable_tun_mode && !isActive
      ? m.error_transparent_proxy_tun_conflict()
      : status.isError
        ? m.settings_clash_transparent_proxy_status_unavailable()
        : status.isPending || (!isActive && status.isFetching)
          ? m.settings_clash_transparent_proxy_status_loading()
          : status.data && !status.data.supported
            ? m.settings_clash_transparent_proxy_status_unsupported()
            : isActive && !applied
              ? m.settings_clash_transparent_proxy_status_pending()
              : undefined;

  const execute = useLockFn(async () => {
    if (disabled || !config.query.data) return;
    setIsSyncing(true);
    try {
      await config.upsert.mutateAsync(
        transparentProxyNextPatch(mode, Boolean(config.query.data.tproxy_port)),
      );
      // The mutation has already refreshed the persisted Clash config. Read
      // the service's applied state afterwards; don't optimistically claim
      // that firewall rules changed when only the config write succeeded.
      await queryClient.invalidateQueries({
        queryKey: [TRANSPARENT_PROXY_STATUS_QUERY_KEY],
      });
    } catch (error) {
      void message(formatError(error), { title: 'Error', kind: 'error' });
    } finally {
      setIsSyncing(false);
    }
  });

  return {
    visible,
    isActive,
    applied,
    isPending,
    disabled,
    reason,
    status: status.data,
    mode,
    execute,
  };
};
