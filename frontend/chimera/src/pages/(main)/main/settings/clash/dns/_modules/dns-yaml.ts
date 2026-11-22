import { stringify } from 'yaml';
import type { DnsDraft } from './dns-draft';

/** The draft as one YAML document, with extra keys inside the DNS table. */
export function dnsDraftToYaml({ dns, hosts }: DnsDraft) {
  const { extra, ...typed } = dns;
  return stringify({ dns: { ...extra, ...typed }, hosts });
}
