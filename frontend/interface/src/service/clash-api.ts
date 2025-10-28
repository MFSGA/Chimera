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

  return {
    deleteConnections,
  };
};
