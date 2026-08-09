import * as m from '@/paraglide/messages';
import {
  SettingsCard,
  SettingsCardContent,
  SettingsGroup,
  SettingsLabel,
} from '../../_modules/settings-card';
import AutoLaunchSwitch from './auto-launch-switch';
import SilentLaunchSwitch from './silent-launch-switch';

export default function SystemBehavior() {
  return (
    <section data-slot="system-launch-container">
      <SettingsLabel>{m.settings_system_proxy_launch_label()}</SettingsLabel>
      <SettingsGroup>
        <SettingsCard>
          <SettingsCardContent>
            <AutoLaunchSwitch />
          </SettingsCardContent>
        </SettingsCard>
        <SettingsCard>
          <SettingsCardContent>
            <SilentLaunchSwitch />
          </SettingsCardContent>
        </SettingsCard>
      </SettingsGroup>
    </section>
  );
}
