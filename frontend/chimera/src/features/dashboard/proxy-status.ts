export type ProxyStatus =
  'system' | 'tun' | 'transparent' | 'occupied' | 'disabled';

type ProxyStatusSource = {
  enableSystemProxy?: boolean | null;
  enableTunMode?: boolean | null;
  transparentProxyActive?: boolean | null;
  systemProxyEnabled?: boolean | null;
  systemProxyServer?: string | null;
  mixedPort?: number | null;
};

export function getProxyStatus({
  enableSystemProxy,
  enableTunMode,
  transparentProxyActive,
  systemProxyEnabled,
  systemProxyServer,
  mixedPort,
}: ProxyStatusSource): ProxyStatus {
  if (enableTunMode) return 'tun';
  if (transparentProxyActive) return 'transparent';

  if (enableSystemProxy && systemProxyEnabled) {
    const port = Number(systemProxyServer?.split(':')[1]);
    return port === mixedPort ? 'system' : 'occupied';
  }

  return 'disabled';
}
