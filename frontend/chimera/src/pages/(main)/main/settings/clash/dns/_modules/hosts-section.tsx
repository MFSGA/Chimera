import type { ControlSource } from '@chimera/interface';
import * as m from '@/paraglide/messages';
import {
  SettingsCard,
  SettingsCardContent,
  SettingsGroup,
  SettingsLabel,
} from '../../../_modules/settings-card';
import { useDnsDraft } from './dns-draft';
import { MappingField } from './list-fields';

export default function HostsSection({
  hostsControl,
}: {
  hostsControl: ControlSource | undefined;
}) {
  const { draft, setHosts, features } = useDnsDraft();
  return (
    <div data-slot="dns-hosts-settings">
      <SettingsLabel>{m.dns_hosts()}</SettingsLabel>
      <SettingsGroup>
        {hostsControl !== 'chimera' && (
          <p
            className="text-on-surface-variant px-2 text-sm"
            data-slot="dns-hosts-profile-control-notice"
          >
            {m.dns_hosts_profile_controls_notice()}
          </p>
        )}
        {!features.list_values && (
          <p className="text-on-surface-variant px-2 text-sm">
            {m.dns_single_target_core()}
          </p>
        )}
        <SettingsCard>
          <SettingsCardContent>
            <MappingField
              label={m.dns_hosts_entries()}
              entries={draft.hosts}
              keyPlaceholder="example.com"
              valuePlaceholder="127.0.0.1"
              onChange={setHosts}
            />
          </SettingsCardContent>
        </SettingsCard>
      </SettingsGroup>
    </div>
  );
}
