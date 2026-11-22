import {
  dnsKeySupported,
  useClashSetting,
  type EnhancedMode,
  type FakeIpFilterMode,
} from '@chimera/interface';
import { Input } from '@chimera/ui/input';
import {
  Select,
  SelectContent,
  SelectItem,
  SelectTrigger,
  SelectValue,
} from '@chimera/ui/select';
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
import { StringListField } from './list-fields';

type BooleanKey = {
  [K in keyof DnsValues]: DnsValues[K] extends boolean ? K : never;
}[keyof DnsValues];

function DnsSwitch({
  field,
  label,
  locked,
}: {
  field: BooleanKey;
  label: string;
  locked?: boolean;
}) {
  const { draft, updateDns, features } = useDnsDraft();
  if (!dnsKeySupported(features, field)) return null;
  return (
    <SettingsCard>
      <SettingsCardContent>
        <ItemContainer>
          <ItemLabel>
            <ItemLabelText>{label}</ItemLabelText>
          </ItemLabel>
          <Switch
            aria-label={label}
            checked={locked || draft.dns[field]}
            disabled={locked}
            onCheckedChange={(value) => updateDns({ [field]: value })}
          />
        </ItemContainer>
      </SettingsCardContent>
    </SettingsCard>
  );
}

function DnsText({
  field,
  label,
  placeholder,
}: {
  field: 'listen' | 'fake-ip-range' | 'fake-ip-range6';
  label: string;
  placeholder: string;
}) {
  const { draft, updateDns, features } = useDnsDraft();
  if (!dnsKeySupported(features, field)) return null;
  return (
    <SettingsCard>
      <SettingsCardContent>
        <Input
          label={label}
          value={draft.dns[field] ?? ''}
          placeholder={placeholder}
          onChange={(event) =>
            updateDns({ [field]: event.target.value.trim() || null })
          }
        />
      </SettingsCardContent>
    </SettingsCard>
  );
}

export default function DnsGeneralSection() {
  const { draft, updateDns, features } = useDnsDraft();
  const tun = useClashSetting('enable_tun_mode');
  return (
    <>
      <div data-slot="dns-general-settings">
        <SettingsLabel>{m.dns_general()}</SettingsLabel>
        <SettingsGroup>
          <DnsSwitch
            field="enable"
            label={m.dns_enabled()}
            locked={Boolean(tun.value)}
          />
          {tun.value && (
            <p className="text-on-surface-variant px-2 text-sm">
              {m.dns_tun_forces_enabled()}
            </p>
          )}
          <SettingsCard>
            <SettingsCardContent>
              <ItemContainer>
                <ItemLabel>
                  <ItemLabelText>{m.dns_enhanced_mode()}</ItemLabelText>
                </ItemLabel>
                <Select
                  variant="chip"
                  value={draft.dns['enhanced-mode']}
                  onValueChange={(value) =>
                    updateDns({ 'enhanced-mode': value as EnhancedMode })
                  }
                >
                  <SelectTrigger aria-label={m.dns_enhanced_mode()}>
                    <SelectValue>{draft.dns['enhanced-mode']}</SelectValue>
                  </SelectTrigger>
                  <SelectContent>
                    {(features.enhanced_mode.kind === 'values'
                      ? features.enhanced_mode.value
                      : []
                    ).map((value) => (
                      <SelectItem key={value} value={value}>
                        {value}
                      </SelectItem>
                    ))}
                  </SelectContent>
                </Select>
              </ItemContainer>
            </SettingsCardContent>
          </SettingsCard>
          <DnsText
            field="listen"
            label={m.dns_listen()}
            placeholder="127.0.0.1:1053"
          />
          <DnsSwitch field="ipv6" label="IPv6" />
          <DnsSwitch field="respect-rules" label={m.dns_respect_rules()} />
          <DnsSwitch field="use-hosts" label={m.dns_use_hosts()} />
          <DnsSwitch
            field="use-system-hosts"
            label={m.dns_use_system_hosts()}
          />
          <DnsSwitch field="prefer-h3" label={m.dns_prefer_h3()} />
        </SettingsGroup>
      </div>
      {draft.dns['enhanced-mode'] === 'fake-ip' && (
        <div data-slot="dns-fake-ip-settings">
          <SettingsLabel>{m.dns_fake_ip()}</SettingsLabel>
          <SettingsGroup>
            <DnsText
              field="fake-ip-range"
              label={m.dns_fake_ip_range()}
              placeholder="198.18.0.1/16"
            />
            <DnsText
              field="fake-ip-range6"
              label={m.dns_fake_ip_range6()}
              placeholder="fdfe:dcba:9876::1/64"
            />
            {dnsKeySupported(features, 'fake-ip-filter-mode') && (
              <SettingsCard>
                <SettingsCardContent>
                  <ItemContainer>
                    <ItemLabel>
                      <ItemLabelText>
                        {m.dns_fake_ip_filter_mode()}
                      </ItemLabelText>
                    </ItemLabel>
                    <Select
                      variant="chip"
                      value={draft.dns['fake-ip-filter-mode']}
                      onValueChange={(value) =>
                        updateDns({
                          'fake-ip-filter-mode': value as FakeIpFilterMode,
                        })
                      }
                    >
                      <SelectTrigger aria-label={m.dns_fake_ip_filter_mode()}>
                        <SelectValue>
                          {draft.dns['fake-ip-filter-mode']}
                        </SelectValue>
                      </SelectTrigger>
                      <SelectContent>
                        {(features.fake_ip_filter_mode.kind === 'values'
                          ? features.fake_ip_filter_mode.value
                          : []
                        ).map((value) => (
                          <SelectItem key={value} value={value}>
                            {value}
                          </SelectItem>
                        ))}
                      </SelectContent>
                    </Select>
                  </ItemContainer>
                </SettingsCardContent>
              </SettingsCard>
            )}
            <SettingsCard>
              <SettingsCardContent>
                <StringListField
                  label={m.dns_fake_ip_filter()}
                  values={draft.dns['fake-ip-filter']}
                  placeholder={
                    draft.dns['fake-ip-filter-mode'] === 'rule'
                      ? 'DOMAIN-SUFFIX,lan,real-ip'
                      : '+.lan'
                  }
                  onChange={(value) => updateDns({ 'fake-ip-filter': value })}
                />
              </SettingsCardContent>
            </SettingsCard>
          </SettingsGroup>
        </div>
      )}
    </>
  );
}
