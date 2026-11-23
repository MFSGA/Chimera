import { dnsKeySupported } from '@chimera/interface';
import { Input } from '@chimera/ui/input';
import { Switch } from '@chimera/ui/switch';
import * as m from '@/paraglide/messages';
import {
  ItemContainer,
  ItemLabel,
  ItemLabelText,
  SettingsCard,
  SettingsCardContent,
  SettingsGroup,
  SettingsLabel,
} from '../../../_modules/settings-card';
import { useDnsDraft, type DnsValues } from './dns-draft';
import { MappingField, StringListField } from './list-fields';

type ServerListKey =
  | 'default-nameserver'
  | 'nameserver'
  | 'proxy-server-nameserver'
  | 'direct-nameserver'
  | 'fallback';

function ServerList({ field, label }: { field: ServerListKey; label: string }) {
  const { draft, updateDns, features } = useDnsDraft();
  if (!dnsKeySupported(features, field)) return null;
  return (
    <SettingsCard data-slot="dns-server-list-card" data-dns-key={field}>
      <SettingsCardContent>
        <StringListField
          label={label}
          values={draft.dns[field]}
          placeholder="https://dns.example/dns-query"
          onChange={(values) =>
            updateDns({ [field]: values } as Partial<DnsValues>)
          }
        />
      </SettingsCardContent>
    </SettingsCard>
  );
}

function FallbackFilterSettings() {
  const { draft, updateDns, features } = useDnsDraft();
  const value = draft.dns['fallback-filter'];
  const update = (patch: Partial<typeof value>) =>
    updateDns({ 'fallback-filter': { ...value, ...patch } });
  return (
    <div data-slot="dns-fallback-filter-settings">
      <SettingsLabel>{m.dns_fallback_filter()}</SettingsLabel>
      <SettingsGroup>
        <SettingsCard>
          <SettingsCardContent>
            <ItemContainer>
              <ItemLabel>
                <ItemLabelText>{m.dns_geoip()}</ItemLabelText>
              </ItemLabel>
              <Switch
                aria-label={m.dns_geoip()}
                checked={value.geoip}
                onCheckedChange={(geoip) => update({ geoip })}
              />
            </ItemContainer>
          </SettingsCardContent>
        </SettingsCard>
        <SettingsCard>
          <SettingsCardContent>
            <Input
              label={m.dns_geoip_code()}
              value={value['geoip-code']}
              onChange={(event) => update({ 'geoip-code': event.target.value })}
            />
          </SettingsCardContent>
        </SettingsCard>
        <SettingsCard>
          <SettingsCardContent>
            <StringListField
              label="IP CIDR"
              values={value.ipcidr}
              onChange={(ipcidr) => update({ ipcidr })}
              placeholder="240.0.0.0/4"
            />
          </SettingsCardContent>
        </SettingsCard>
        <SettingsCard>
          <SettingsCardContent>
            <StringListField
              label="Domain"
              values={value.domain}
              onChange={(domain) => update({ domain })}
              placeholder="+.google.com"
            />
          </SettingsCardContent>
        </SettingsCard>
        {features.fallback_filter_geosite && (
          <SettingsCard>
            <SettingsCardContent>
              <StringListField
                label="Geosite"
                values={value.geosite}
                onChange={(geosite) => update({ geosite })}
                placeholder="gfw"
              />
            </SettingsCardContent>
          </SettingsCard>
        )}
      </SettingsGroup>
    </div>
  );
}

export default function DnsUpstreamSection() {
  const { draft, updateDns, features } = useDnsDraft();
  return (
    <>
      <div data-slot="dns-upstream-settings">
        <SettingsLabel>{m.dns_upstream()}</SettingsLabel>
        <SettingsGroup>
          <ServerList
            field="default-nameserver"
            label={m.dns_default_nameservers()}
          />
          <ServerList field="nameserver" label={m.dns_nameservers()} />
          <ServerList
            field="proxy-server-nameserver"
            label={m.dns_proxy_nameservers()}
          />
          <ServerList
            field="direct-nameserver"
            label={m.dns_direct_nameservers()}
          />
          {dnsKeySupported(features, 'direct-nameserver-follow-policy') && (
            <SettingsCard>
              <SettingsCardContent>
                <ItemContainer>
                  <ItemLabel>
                    <ItemLabelText>
                      {m.dns_direct_follow_policy()}
                    </ItemLabelText>
                  </ItemLabel>
                  <Switch
                    checked={draft.dns['direct-nameserver-follow-policy']}
                    onCheckedChange={(value) =>
                      updateDns({ 'direct-nameserver-follow-policy': value })
                    }
                  />
                </ItemContainer>
              </SettingsCardContent>
            </SettingsCard>
          )}
          <ServerList field="fallback" label={m.dns_fallback()} />
          <SettingsCard data-slot="dns-nameserver-policy-card">
            <SettingsCardContent>
              <MappingField
                label={m.dns_nameserver_policy()}
                entries={draft.dns['nameserver-policy']}
                keyPlaceholder="geosite:cn"
                valuePlaceholder="223.5.5.5, 119.29.29.29"
                onChange={(entries) =>
                  updateDns({ 'nameserver-policy': entries })
                }
              />
            </SettingsCardContent>
          </SettingsCard>
        </SettingsGroup>
      </div>
      {draft.dns.fallback.length > 0 && <FallbackFilterSettings />}
    </>
  );
}
