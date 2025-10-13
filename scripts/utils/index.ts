import { ProxyAgent } from 'undici';

export const HTTP_PROXY =
  process.env.HTTP_PROXY ||
  process.env.http_proxy ||
  process.env.HTTPS_PROXY ||
  process.env.https_proxy;

export function getProxyAgent() {
  if (HTTP_PROXY) {
    return new ProxyAgent(HTTP_PROXY);
  }

  return undefined;
}
