import { locales, type Locale } from '../../paraglide/runtime';

export type SystemProxyDeepLinkMode = 'on' | 'off' | 'toggle';

export type DeepLinkCommand =
  | {
      type: 'install-config';
      url: string;
      name?: string;
    }
  | {
      type: 'subscribe-remote-profile';
      url?: string;
      name?: string;
      description?: string;
    }
  | {
      type: 'system-proxy';
      mode: SystemProxyDeepLinkMode;
    }
  | {
      type: 'language';
      locale: Locale;
    };

const normalizeSchemePath = (url: URL) => {
  let pathname = `${url.hostname || ''}${url.pathname || ''}`;

  if (pathname.endsWith('/')) {
    pathname = pathname.slice(0, -1);
  }

  if (pathname.startsWith('//')) {
    pathname = pathname.slice(2);
  }

  return pathname;
};

const decodeSearchParam = (value: string | null) => {
  if (!value) return undefined;

  try {
    return decodeURIComponent(value);
  } catch {
    return value;
  }
};

const parseInstallConfig = (url: URL): DeepLinkCommand => {
  const subscribeUrl = url.searchParams.get('url');
  if (!subscribeUrl) {
    throw new Error('install-config requires a url parameter');
  }

  const parsedSubscribeUrl = new URL(subscribeUrl);
  if (!['http:', 'https:'].includes(parsedSubscribeUrl.protocol)) {
    throw new Error('subscription URL must use http or https');
  }

  return {
    type: 'install-config',
    url: subscribeUrl,
    name: decodeSearchParam(url.searchParams.get('name')),
  };
};

const parseSystemProxy = (url: URL): DeepLinkCommand => {
  const mode = url.searchParams.get('mode');
  if (mode !== 'on' && mode !== 'off' && mode !== 'toggle') {
    throw new Error('system-proxy mode must be on, off, or toggle');
  }

  return { type: 'system-proxy', mode };
};

const parseLanguage = (url: URL): DeepLinkCommand => {
  const locale = url.searchParams.get('locale');
  if (!locale || !locales.includes(locale as Locale)) {
    throw new Error(`unsupported locale: ${locale ?? '<missing>'}`);
  }

  return { type: 'language', locale: locale as Locale };
};

export const parseDeepLink = (raw: string): DeepLinkCommand => {
  const url = new URL(raw);
  if (url.protocol !== 'chimera:') {
    throw new Error(`unsupported deep-link protocol: ${url.protocol}`);
  }

  switch (normalizeSchemePath(url)) {
    case 'install-config':
      return parseInstallConfig(url);
    case 'subscribe-remote-profile':
      return {
        type: 'subscribe-remote-profile',
        url: url.searchParams.get('url') || undefined,
        name: decodeSearchParam(url.searchParams.get('name')),
        description: decodeSearchParam(url.searchParams.get('desc')),
      };
    case 'system-proxy':
      return parseSystemProxy(url);
    case 'language':
      return parseLanguage(url);
    default:
      throw new Error(
        `unsupported deep-link command: ${normalizeSchemePath(url)}`,
      );
  }
};
