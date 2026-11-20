import type { TransparentProxyMode } from '@chimera/interface';

export type TransparentProxyAdmission = {
  supportedHost: boolean;
  supportedCore: boolean;
  serviceModeEnabled: boolean;
  statusSupported: boolean;
  tunEnabled: boolean;
  configReady: boolean;
  mode: TransparentProxyMode;
};

/** The same fail-closed enable guards as the ref's proxy settings hook.
 * Turning off an already configured transparent proxy always remains possible. */
export function canToggleTransparentProxy({
  supportedHost,
  supportedCore,
  serviceModeEnabled,
  statusSupported,
  tunEnabled,
  configReady,
  mode,
}: TransparentProxyAdmission): boolean {
  if (!supportedHost || !supportedCore || !configReady) return false;
  if (mode !== 'disabled') return true;
  return serviceModeEnabled && statusSupported && !tunEnabled;
}

export function transparentProxyNextPatch(
  mode: TransparentProxyMode,
  hasTproxyPort: boolean,
) {
  return {
    transparent_proxy: {
      mode: mode !== 'disabled' ? ('disabled' as const) : ('tproxy' as const),
    },
    ...(mode === 'disabled' && !hasTproxyPort
      ? { tproxy_port: { kind: 'fixed' as const, start_port: 7894 } }
      : {}),
  };
}
