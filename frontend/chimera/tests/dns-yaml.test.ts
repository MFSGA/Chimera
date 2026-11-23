import assert from 'node:assert/strict';
import test from 'node:test';
import { parse } from 'yaml';
import type { DnsDraft } from '../src/pages/(main)/main/settings/clash/dns/_modules/dns-draft';
import { dnsDraftToYaml } from '../src/pages/(main)/main/settings/clash/dns/_modules/dns-yaml';

test('typed DNS values win when an extra key collides during YAML export', () => {
  const draft = {
    dns: {
      nameserver: ['https://1.1.1.1/dns-query'],
      extra: {
        nameserver: ['https://profile.example/dns-query'],
        'custom-setting': true,
      },
    },
    hosts: {},
  } as unknown as DnsDraft;

  const document = parse(dnsDraftToYaml(draft));
  assert.deepEqual(document.dns.nameserver, ['https://1.1.1.1/dns-query']);
  assert.equal(document.dns['custom-setting'], true);
});
