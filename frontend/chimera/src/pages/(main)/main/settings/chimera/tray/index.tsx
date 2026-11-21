import { createFileRoute } from '@tanstack/react-router';
import * as m from '@/paraglide/messages';
import TrayProxiesSelector from '../_modules/tray-proxies-selector';
import WindowCloseBehaviorCards from '../_modules/window-close-behavior';
import { SettingsGroup, SettingsLabel } from '../../_modules/settings-card';
import { SettingsTitle } from '../../_modules/settings-title';

export const Route = createFileRoute('/(main)/main/settings/chimera/tray/')({
  component: RouteComponent,
});

function RouteComponent() {
  return (
    <>
      <SettingsTitle>{m.settings_label_tray()}</SettingsTitle>
      <div className="space-y-4 px-4 pb-4">
        <div data-slot="tray-settings-container">
          <SettingsLabel>{m.settings_chimera_tray()}</SettingsLabel>
          <SettingsGroup>
            <TrayProxiesSelector />
          </SettingsGroup>
        </div>
        <div data-slot="window-settings-container">
          <SettingsLabel>{m.settings_chimera_window_close()}</SettingsLabel>
          <SettingsGroup>
            <WindowCloseBehaviorCards />
          </SettingsGroup>
        </div>
      </div>
    </>
  );
}
