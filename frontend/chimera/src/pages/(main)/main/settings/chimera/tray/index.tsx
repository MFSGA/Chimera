import { createFileRoute } from '@tanstack/react-router';
import { PageTitle } from '@/components/page-title';
import * as m from '@/paraglide/messages';
import TrayProxiesSelector from '../_modules/tray-proxies-selector';
import WindowCloseBehaviorCards from '../_modules/window-close-behavior';
import { SettingsGroup, SettingsLabel } from '../../_modules/settings-card';

export const Route = createFileRoute('/(main)/main/settings/chimera/tray/')({
  component: RouteComponent,
  staticData: {
    crumb: {
      title: () => m.settings_label_tray(),
      parent: () => ({ to: '/main/settings' }),
    },
  },
});

function RouteComponent() {
  return (
    <>
      <PageTitle legacySlot="settings-title" />
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
