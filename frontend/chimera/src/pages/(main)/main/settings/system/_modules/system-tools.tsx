import * as m from '@/paraglide/messages';
import { SettingsGroup, SettingsLabel } from '../../_modules/settings-card';
import UwpToolsButton from './uwp-tools-button';

export default function SystemTools() {
  return (
    <section data-slot="system-tools-container">
      <SettingsLabel>
        {m.settings_system_proxy_windows_tools_label()}
      </SettingsLabel>

      <SettingsGroup>
        <UwpToolsButton />
      </SettingsGroup>
    </section>
  );
}
