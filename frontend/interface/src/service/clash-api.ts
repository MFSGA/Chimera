import { ofetch } from 'ofetch';
import { useMemo } from 'react';
// import type { ProxyGroupItem, SubscriptionInfo } from '../ipc/bindings'
import { useClashInfo } from '../ipc/use-clash-info';

const prepareServer = (server?: string) => {
  if (server?.startsWith(':')) {
    return `127.0.0.1${server}`;
  } else if (server && /^\d+$/.test(server)) {
    return `127.0.0.1:${server}`;
  } else {
    return server;
  }
};

export const useClashAPI = () => {
  const { data } = useClashInfo();

  const request = useMemo(() => {
    return ofetch.create({
      baseURL: `http://${prepareServer(data?.server)}`,
      headers: data?.secret
        ? { Authorization: `Bearer ${data?.secret}` }
        : undefined,
    });
  }, [data]);

  const deleteConnections = async (id?: string) => {
    const url = id ? `/connections/${id}` : '/connections';

    return await request(url, {
      method: 'DELETE',
    });
  };

  /**
   * Fetches Clash configurations from the server.
   */
  const configs = async () => {
    return await request<ClashConfig>('/configs');
  };

  /**
   * Update basic configuration; data must be sent in the format '{"mixed-port": 7890}',
   * modified as needed for the configuration items to be updated.
   */
  const patchConfigs = async (config: Partial<ClashConfig>) => {
    return await request<ClashConfig>('/configs', {
      method: 'PATCH',
      body: config,
    });
  };

  return {
    deleteConnections,
    configs,
    patchConfigs,
  };
};

export interface ClashConfig {
  port: number;
  mode: string;
  ipv6: boolean;
  'socket-port': number;
  'allow-lan': boolean;
  'log-level': string;
  'mixed-port': number;
  'redir-port': number;
  'socks-port': number;
  'tproxy-port': number;
  'external-controller': string;
  secret: string;
}
