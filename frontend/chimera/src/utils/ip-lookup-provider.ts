import type { IpLookupProvider } from '@chimera/interface';

/** Service names are brands and stay untranslated. */
export const IP_LOOKUP_PROVIDER_NAMES: Record<IpLookupProvider, string> = {
  dnspod: 'DNSPod',
  ip_sb: 'IP.SB',
  ipify: 'ipify',
  ident_me: 'ident.me',
};
