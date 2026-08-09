import * as m from '@/paraglide/messages';
import {
  SettingsCard,
  SettingsCardContent,
  SettingsGroup,
  SettingsLabel,
} from '../../_modules/settings-card';
import SystemServiceControl from './system-service-control';
import SystemServiceSwitch from './system-service-switch';

export default function SystemService() {
  return (
    <section data-slot="system-service-container">
      <SettingsLabel>
        {m.settings_system_proxy_system_service_ctrl_label()}
      </SettingsLabel>
      <SettingsGroup>
        <SettingsCard>
          <SettingsCardContent>
            <SystemServiceSwitch />
          </SettingsCardContent>
        </SettingsCard>
        <SystemServiceControl />
      </SettingsGroup>
    </section>
  );
}
