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
    const server = prepareServer(data?.server);

    if (!server) {
      return null;
    }

    return ofetch.create({
      baseURL: `http://${server}`,
      headers: data?.secret
        ? { Authorization: `Bearer ${data?.secret}` }
        : undefined,
    });
  }, [data?.secret, data?.server]);

  const getRequest = () => {
    if (!request) {
      throw new Error('Clash controller is not ready');
    }

    return request;
  };

  const deleteConnections = async (id?: string) => {
    const url = id ? `/connections/${id}` : '/connections';

    return await getRequest()(url, {
      method: 'DELETE',
    });
  };

  /**
   * Fetches Clash configurations from the server.
   */
  const configs = async () => {
    return await getRequest()<ClashConfig>('/configs');
  };

  /**
   * Update basic configuration; data must be sent in the format '{"mixed-port": 7890}',
   * modified as needed for the configuration items to be updated.
   */
  const patchConfigs = async (config: Partial<ClashConfig>) => {
    return await getRequest()<ClashConfig>('/configs', {
      method: 'PATCH',
      body: config,
    });
  };

  const proxiesDelay = async (name: string, options?: ClashDelayOptions) => {
    return await getRequest()<{ delay: number }>(
      `/proxies/${encodeURIComponent(name)}/delay`,
      {
        params: {
          timeout: options?.timeout || 10000,
          url: options?.url || 'http://www.gstatic.com/generate_204',
        },
      },
    );
  };

  const groupDelay = async (group: string, options?: ClashDelayOptions) => {
    return await getRequest()<Record<string, number>>(
      `/group/${encodeURIComponent(group)}/delay`,
      {
        params: {
          timeout: options?.timeout || 10000,
          url: options?.url || 'http://www.gstatic.com/generate_204',
        },
      },
    );
  };

  const rules = async () => {
    return await getRequest()<{
      rules: ClashRule[];
    }>('/rules');
  };

  const version = async () => {
    return await getRequest()<ClashVersion>('/version');
  };

  return {
    isReady: Boolean(request),
    deleteConnections,
    configs,
    patchConfigs,
    proxiesDelay,
    groupDelay,
    rules,
    version,
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

export type ClashDelayOptions = {
  url?: string;
  timeout?: number;
};

export type ClashVersion = {
  premium?: boolean;
  meta?: boolean;
  version: string;
};

export type ClashRule = {
  type: string;
  payload: string;
  proxy: string;
};
