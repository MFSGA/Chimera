import type { Locale } from '@/paraglide/runtime';
import type { DeepLinkCommand, SystemProxyDeepLinkMode } from './parser';

export type SettingsDeepLinkCommand = Extract<
  DeepLinkCommand,
  { type: 'system-proxy' | 'language' }
>;

export interface DeepLinkSettingsGateway {
  getSystemProxyEnabled: () => Promise<boolean>;
  setSystemProxyEnabled: (enabled: boolean) => Promise<void>;
  setLanguage: (locale: Locale) => Promise<void>;
}

export const resolveSystemProxyEnabled = (
  current: boolean,
  mode: SystemProxyDeepLinkMode,
) => {
  switch (mode) {
    case 'on':
      return true;
    case 'off':
      return false;
    case 'toggle':
      return !current;
  }
};

const applySystemProxy = async (
  mode: SystemProxyDeepLinkMode,
  gateway: DeepLinkSettingsGateway,
) => {
  const current = await gateway.getSystemProxyEnabled();
  const next = resolveSystemProxyEnabled(current, mode);
  if (next === current) return false;

  await gateway.setSystemProxyEnabled(next);
  return true;
};

export const applySettingsDeepLink = async (
  command: SettingsDeepLinkCommand,
  gateway: DeepLinkSettingsGateway,
) => {
  switch (command.type) {
    case 'system-proxy':
      return applySystemProxy(command.mode, gateway);
    case 'language':
      await gateway.setLanguage(command.locale);
      return true;
  }
};
